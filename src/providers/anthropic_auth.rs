//! Credentials for the Anthropic provider.
//!
//! Either a static API key, or Workload Identity Federation: an OIDC identity
//! token from the environment is exchanged at `/v1/oauth/token` for a
//! short-lived access token, so no long-lived secret has to be stored.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::error::{Error, Result};

const API_KEY: &str = "ANTHROPIC_API_KEY";
const FEDERATION_RULE_ID: &str = "ANTHROPIC_FEDERATION_RULE_ID";
const ORGANIZATION_ID: &str = "ANTHROPIC_ORGANIZATION_ID";
const SERVICE_ACCOUNT_ID: &str = "ANTHROPIC_SERVICE_ACCOUNT_ID";
const WORKSPACE_ID: &str = "ANTHROPIC_WORKSPACE_ID";
const IDENTITY_TOKEN_FILE: &str = "ANTHROPIC_IDENTITY_TOKEN_FILE";
const IDENTITY_TOKEN: &str = "ANTHROPIC_IDENTITY_TOKEN";
const IDENTITY_TOKEN_AUDIENCE: &str = "COMMUNIQUE_ANTHROPIC_IDENTITY_TOKEN_AUDIENCE";
const ACTIONS_ID_TOKEN_REQUEST_URL: &str = "ACTIONS_ID_TOKEN_REQUEST_URL";
const ACTIONS_ID_TOKEN_REQUEST_TOKEN: &str = "ACTIONS_ID_TOKEN_REQUEST_TOKEN";

const DEFAULT_AUDIENCE: &str = "https://api.anthropic.com";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Start trying to replace a cached token this long before it expires.
const ADVISORY_REFRESH: Duration = Duration::from_secs(120);
/// Stop serving a cached token this close to expiry, even if a refresh failed.
const MANDATORY_REFRESH: Duration = Duration::from_secs(30);
const EXCHANGE_ATTEMPTS: u32 = 3;
/// Deadline for each authentication request, including reading the response,
/// so a stalled endpoint cannot hold up every Messages request.
const AUTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub enum AnthropicAuth {
    ApiKey(String),
    Federation(Box<Federation>),
}

/// Where the identity provider's JWT comes from. Read again for every
/// exchange, because identity tokens are short-lived and usually single-use.
enum IdentitySource {
    File(PathBuf),
    Literal(String),
    /// Ask the GitHub Actions runner for a fresh OIDC token (needs the
    /// `id-token: write` permission).
    GitHubActions {
        request_url: String,
        request_token: String,
        audience: String,
    },
}

pub struct Federation {
    rule_id: String,
    organization_id: String,
    service_account_id: Option<String>,
    workspace_id: Option<String>,
    source: IdentitySource,
    cached: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct GitHubIdToken {
    value: String,
}

impl AnthropicAuth {
    /// An API key wins when both are configured, matching Anthropic's SDKs.
    pub fn from_env() -> Result<Self> {
        Self::resolve(|name| std::env::var(name).ok())
    }

    fn resolve(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        // CI often passes an unset secret through as an empty string.
        let get = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());

        if let Some(key) = get(API_KEY) {
            return Ok(Self::ApiKey(key));
        }

        let (rule_id, organization_id) = match (get(FEDERATION_RULE_ID), get(ORGANIZATION_ID)) {
            (Some(rule_id), Some(organization_id)) => (rule_id, organization_id),
            (None, None) => {
                return Err(Error::Llm(format!(
                    "{API_KEY} not set (alternatively, configure workload identity federation \
                     with {FEDERATION_RULE_ID} and {ORGANIZATION_ID})"
                )));
            }
            _ => {
                return Err(Error::Llm(format!(
                    "workload identity federation needs both {FEDERATION_RULE_ID} and \
                     {ORGANIZATION_ID}"
                )));
            }
        };

        let source = if let Some(path) = get(IDENTITY_TOKEN_FILE) {
            IdentitySource::File(path.into())
        } else if let Some(token) = get(IDENTITY_TOKEN) {
            IdentitySource::Literal(token)
        } else if let (Some(request_url), Some(request_token)) = (
            get(ACTIONS_ID_TOKEN_REQUEST_URL),
            get(ACTIONS_ID_TOKEN_REQUEST_TOKEN),
        ) {
            IdentitySource::GitHubActions {
                request_url,
                request_token,
                audience: get(IDENTITY_TOKEN_AUDIENCE).unwrap_or_else(|| DEFAULT_AUDIENCE.into()),
            }
        } else {
            return Err(Error::Llm(format!(
                "workload identity federation has no identity token: set {IDENTITY_TOKEN_FILE} \
                 or {IDENTITY_TOKEN}, or in GitHub Actions grant the job `id-token: write`"
            )));
        };

        Ok(Self::Federation(Box::new(Federation {
            rule_id,
            organization_id,
            service_account_id: get(SERVICE_ACCOUNT_ID),
            workspace_id: get(WORKSPACE_ID),
            source,
            cached: Mutex::new(None),
        })))
    }
}

#[cfg(test)]
impl AnthropicAuth {
    pub fn federation_for_test(rule_id: &str, organization_id: &str, identity_token: &str) -> Self {
        Self::Federation(Box::new(Federation {
            rule_id: rule_id.into(),
            organization_id: organization_id.into(),
            service_account_id: None,
            workspace_id: None,
            source: IdentitySource::Literal(identity_token.into()),
            cached: Mutex::new(None),
        }))
    }
}

impl IdentitySource {
    async fn assertion(&self, client: &reqwest::Client) -> Result<String> {
        let token = match self {
            Self::File(path) => std::fs::read_to_string(path).map_err(|e| {
                Error::Llm(format!(
                    "failed to read {IDENTITY_TOKEN_FILE} ({}): {e}",
                    path.display()
                ))
            })?,
            Self::Literal(token) => token.clone(),
            Self::GitHubActions {
                request_url,
                request_token,
                audience,
            } => {
                let mut url = reqwest::Url::parse(request_url).map_err(|e| {
                    Error::Llm(format!("invalid {ACTIONS_ID_TOKEN_REQUEST_URL}: {e}"))
                })?;
                url.query_pairs_mut().append_pair("audience", audience);
                fetch_github_token(client, url, request_token).await?
            }
        };
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::Llm(
                "workload identity federation identity token is empty".into(),
            ));
        }
        Ok(token.to_string())
    }
}

/// Fetch an OIDC token from the Actions runner. Transient failures (connection
/// errors, timeouts while reading the body, 429/5xx) are retried; a response
/// that arrives but is rejected or malformed is not.
async fn fetch_github_token(
    client: &reqwest::Client,
    url: reqwest::Url,
    request_token: &str,
) -> Result<String> {
    let mut attempt = 1;
    loop {
        let result = async {
            let resp = client
                .get(url.clone())
                .bearer_auth(request_token)
                .timeout(AUTH_REQUEST_TIMEOUT)
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Ok(Err((status, body)));
            }
            Ok::<_, reqwest::Error>(Ok(resp.bytes().await?))
        }
        .await;

        let error = match result {
            Ok(Ok(bytes)) => {
                return serde_json::from_slice::<GitHubIdToken>(&bytes)
                    .map(|t| t.value)
                    .map_err(|e| {
                        Error::Llm(format!("invalid GitHub Actions OIDC token response: {e}"))
                    });
            }
            Ok(Err((status, body))) => {
                let error = Error::Llm(format!(
                    "failed to fetch a GitHub Actions OIDC token: {status}: {body}"
                ));
                if !(status.is_server_error() || status.as_u16() == 429) {
                    return Err(error);
                }
                error
            }
            Err(e) if e.is_connect() || e.is_timeout() || e.is_body() => e.into(),
            Err(e) => return Err(e.into()),
        };
        if attempt == EXCHANGE_ATTEMPTS {
            return Err(error);
        }
        log::warn!("{error} (attempt {attempt}/{EXCHANGE_ATTEMPTS}), retrying");
        tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
        attempt += 1;
    }
}

impl Federation {
    /// A valid access token, exchanging the identity token when none is cached
    /// or the cached one is about to expire.
    pub async fn access_token(&self, client: &reqwest::Client, base_url: &str) -> Result<String> {
        let mut cached = self.cached.lock().await;
        let remaining = |t: &CachedToken| t.expires_at.saturating_duration_since(Instant::now());

        if let Some(t) = cached.as_ref().filter(|t| remaining(t) > ADVISORY_REFRESH) {
            return Ok(t.token.clone());
        }

        match self.exchange(client, base_url).await {
            Ok(fresh) => {
                let token = fresh.token.clone();
                *cached = Some(fresh);
                Ok(token)
            }
            Err(e) => match cached.as_ref().filter(|t| remaining(t) > MANDATORY_REFRESH) {
                Some(t) => {
                    log::warn!("could not refresh the Anthropic access token, reusing it: {e}");
                    Ok(t.token.clone())
                }
                None => Err(e),
            },
        }
    }

    /// Exchange for a new access token even if the cached one looks valid,
    /// e.g. after the API rejected it.
    pub async fn force_refresh(&self, client: &reqwest::Client, base_url: &str) -> Result<String> {
        let mut cached = self.cached.lock().await;
        let fresh = self.exchange(client, base_url).await?;
        let token = fresh.token.clone();
        *cached = Some(fresh);
        Ok(token)
    }

    async fn exchange(&self, client: &reqwest::Client, base_url: &str) -> Result<CachedToken> {
        let mut attempt = 1;
        loop {
            // A fresh assertion per attempt: a JWT with a `jti` claim can only
            // be exchanged once, so resending the same one would be rejected.
            let assertion = self.source.assertion(client).await?;
            let mut body = json!({
                "grant_type": GRANT_TYPE,
                "assertion": assertion,
                "federation_rule_id": self.rule_id,
                "organization_id": self.organization_id,
            });
            if let Some(id) = &self.service_account_id {
                body["service_account_id"] = json!(id);
            }
            if let Some(id) = &self.workspace_id {
                body["workspace_id"] = json!(id);
            }

            let requested_at = Instant::now();
            let result = client
                .post(format!("{base_url}/v1/oauth/token"))
                .json(&body)
                .timeout(AUTH_REQUEST_TIMEOUT)
                .send()
                .await;

            let error = match result {
                Ok(resp) if resp.status().is_success() => {
                    let token: TokenResponse = resp.json().await?;
                    return Ok(CachedToken {
                        token: token.access_token,
                        expires_at: requested_at + Duration::from_secs(token.expires_in),
                    });
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    let error = Error::Llm(format!(
                        "Anthropic workload identity token exchange failed: {status}: {body}\n\
                         The deny reason is listed at \
                         https://platform.claude.com/settings/workload-identity-federation?tab=history"
                    ));
                    if !(status.is_server_error() || status.as_u16() == 429) {
                        return Err(error);
                    }
                    error
                }
                Err(e) => e.into(),
            };

            if attempt == EXCHANGE_ATTEMPTS {
                return Err(error);
            }
            log::warn!("{error} (attempt {attempt}/{EXCHANGE_ATTEMPTS}), retrying");
            tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
            attempt += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wiremock::matchers::{bearer_token, body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn resolve(vars: &[(&str, &str)]) -> Result<AnthropicAuth> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        AnthropicAuth::resolve(|name| vars.get(name).cloned())
    }

    fn federation(vars: &[(&str, &str)]) -> Box<Federation> {
        match resolve(vars) {
            Ok(AnthropicAuth::Federation(f)) => f,
            Ok(AnthropicAuth::ApiKey(_)) => panic!("expected federation, got an API key"),
            Err(e) => panic!("expected federation, got error: {e}"),
        }
    }

    fn error(vars: &[(&str, &str)]) -> String {
        match resolve(vars) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    const RULE: (&str, &str) = (FEDERATION_RULE_ID, "fdrl_1");
    const ORG: (&str, &str) = (ORGANIZATION_ID, "org-uuid");

    fn token_response(token: &str, expires_in: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": token,
            "token_type": "Bearer",
            "expires_in": expires_in,
        }))
    }

    #[test]
    fn api_key_wins_over_federation() {
        let auth = resolve(&[(API_KEY, "sk-ant-key"), RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        assert!(matches!(auth, Ok(AnthropicAuth::ApiKey(k)) if k == "sk-ant-key"));
    }

    #[test]
    fn empty_api_key_falls_through_to_federation() {
        let f = federation(&[(API_KEY, ""), RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        assert_eq!(f.rule_id, "fdrl_1");
        assert_eq!(f.organization_id, "org-uuid");
        assert!(f.service_account_id.is_none());
        assert!(f.workspace_id.is_none());
        assert!(matches!(&f.source, IdentitySource::Literal(t) if t == "jwt"));
    }

    #[test]
    fn nothing_configured_names_the_api_key() {
        assert!(error(&[]).contains("ANTHROPIC_API_KEY not set"));
    }

    #[test]
    fn partial_federation_config_is_an_error() {
        assert!(error(&[RULE, (IDENTITY_TOKEN, "jwt")]).contains("needs both"));
        assert!(error(&[RULE, ORG]).contains("no identity token"));
    }

    #[test]
    fn identity_token_file_wins_over_ambient_github_actions() {
        let f = federation(&[
            RULE,
            ORG,
            (IDENTITY_TOKEN_FILE, "/tmp/jwt"),
            (ACTIONS_ID_TOKEN_REQUEST_URL, "https://example.com/token"),
            (ACTIONS_ID_TOKEN_REQUEST_TOKEN, "runner-token"),
        ]);
        assert!(matches!(&f.source, IdentitySource::File(p) if p.to_str() == Some("/tmp/jwt")));
    }

    #[tokio::test]
    async fn exchanges_identity_token_file_and_caches_the_access_token() {
        let server = MockServer::start().await;
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "file-jwt\n").unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .and(body_partial_json(json!({
                "grant_type": GRANT_TYPE,
                "assertion": "file-jwt",
                "federation_rule_id": "fdrl_1",
                "organization_id": "org-uuid",
                "service_account_id": "svac_1",
                "workspace_id": "wrkspc_1",
            })))
            .respond_with(token_response("sk-ant-oat01-a", 3600))
            .expect(1)
            .mount(&server)
            .await;

        let f = federation(&[
            RULE,
            ORG,
            (SERVICE_ACCOUNT_ID, "svac_1"),
            (WORKSPACE_ID, "wrkspc_1"),
            (IDENTITY_TOKEN_FILE, file.path().to_str().unwrap()),
        ]);
        let client = reqwest::Client::new();
        for _ in 0..2 {
            let token = f.access_token(&client, &server.uri()).await.unwrap();
            assert_eq!(token, "sk-ant-oat01-a");
        }
    }

    #[tokio::test]
    async fn omits_unset_optional_ids_from_the_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(token_response("sk-ant-oat01-a", 3600))
            .mount(&server)
            .await;

        let f = federation(&[RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        f.access_token(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = requests[0].body_json().unwrap();
        assert!(body.get("service_account_id").is_none());
        assert!(body.get("workspace_id").is_none());
    }

    #[tokio::test]
    async fn fetches_a_fresh_github_actions_token_for_each_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .and(query_param("api-version", "2.0"))
            .and(query_param("audience", DEFAULT_AUDIENCE))
            .and(bearer_token("runner-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": "gha-jwt" })))
            .expect(2)
            .mount(&server)
            .await;
        // Expires inside the refresh window, so the second call exchanges again.
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .and(body_partial_json(json!({ "assertion": "gha-jwt" })))
            .respond_with(token_response("sk-ant-oat01-a", 60))
            .expect(2)
            .mount(&server)
            .await;

        let request_url = format!("{}/idtoken?api-version=2.0", server.uri());
        let f = federation(&[
            RULE,
            ORG,
            (ACTIONS_ID_TOKEN_REQUEST_URL, &request_url),
            (ACTIONS_ID_TOKEN_REQUEST_TOKEN, "runner-token"),
        ]);
        let client = reqwest::Client::new();
        for _ in 0..2 {
            f.access_token(&client, &server.uri()).await.unwrap();
        }
    }

    #[tokio::test]
    async fn retries_a_transient_github_actions_token_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": "gha-jwt" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .and(body_partial_json(json!({ "assertion": "gha-jwt" })))
            .respond_with(token_response("sk-ant-oat01-a", 3600))
            .mount(&server)
            .await;

        let request_url = format!("{}/idtoken", server.uri());
        let f = federation(&[
            RULE,
            ORG,
            (ACTIONS_ID_TOKEN_REQUEST_URL, &request_url),
            (ACTIONS_ID_TOKEN_REQUEST_TOKEN, "runner-token"),
        ]);
        let token = f
            .access_token(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap();
        assert_eq!(token, "sk-ant-oat01-a");
    }

    #[tokio::test]
    async fn retries_a_504_when_fetching_the_github_actions_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .respond_with(ResponseTemplate::new(504))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "value": "gha-jwt" }))
                    .set_delay(Duration::from_millis(200)),
            )
            .mount(&server)
            .await;

        let url = reqwest::Url::parse(&format!("{}/idtoken", server.uri())).unwrap();
        let value = fetch_github_token(&reqwest::Client::new(), url, "runner-token")
            .await
            .unwrap();
        assert_eq!(value, "gha-jwt");
    }

    #[tokio::test]
    async fn does_not_retry_a_rejected_github_actions_token_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/idtoken"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let url = reqwest::Url::parse(&format!("{}/idtoken", server.uri())).unwrap();
        let err = fetch_github_token(&reqwest::Client::new(), url, "runner-token")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn uses_the_configured_github_actions_audience() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("audience", "my-audience"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": "gha-jwt" })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(token_response("sk-ant-oat01-a", 3600))
            .mount(&server)
            .await;

        let request_url = format!("{}/idtoken?api-version=2.0", server.uri());
        let f = federation(&[
            RULE,
            ORG,
            (ACTIONS_ID_TOKEN_REQUEST_URL, &request_url),
            (ACTIONS_ID_TOKEN_REQUEST_TOKEN, "runner-token"),
            (IDENTITY_TOKEN_AUDIENCE, "my-audience"),
        ]);
        f.access_token(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn keeps_a_still_valid_token_when_refresh_fails() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(token_response("sk-ant-oat01-a", 100))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Authentication failed"))
            .mount(&server)
            .await;

        let f = federation(&[RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        let client = reqwest::Client::new();
        for _ in 0..2 {
            let token = f.access_token(&client, &server.uri()).await.unwrap();
            assert_eq!(token, "sk-ant-oat01-a");
        }
    }

    #[tokio::test]
    async fn denied_exchange_is_not_retried_and_points_at_the_history_page() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Authentication failed"))
            .expect(1)
            .mount(&server)
            .await;

        let f = federation(&[RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        let err = f
            .access_token(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("401"), "{err}");
        assert!(err.contains("Authentication failed"), "{err}");
        assert!(err.contains("tab=history"), "{err}");
    }

    #[tokio::test]
    async fn retries_a_transient_exchange_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(token_response("sk-ant-oat01-a", 3600))
            .mount(&server)
            .await;

        let f = federation(&[RULE, ORG, (IDENTITY_TOKEN, "jwt")]);
        let token = f
            .access_token(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap();
        assert_eq!(token, "sk-ant-oat01-a");
    }

    #[tokio::test]
    async fn missing_identity_token_file_is_reported() {
        let f = federation(&[RULE, ORG, (IDENTITY_TOKEN_FILE, "/nonexistent/jwt")]);
        let err = f
            .access_token(&reqwest::Client::new(), "http://localhost")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("/nonexistent/jwt"), "{err}");
    }
}
