# GitHub Actions

communiqué integrates naturally into CI/CD pipelines. This guide covers the most common GitHub Actions patterns.

## Prerequisites

Add your LLM provider secret to your repository (Settings > Secrets and variables > Actions):

| Secret | Purpose |
|--------|---------|
| `ANTHROPIC_API_KEY` | API key for Claude models |
| `OPENAI_API_KEY` | API key for OpenAI-compatible providers |
| `GITHUB_TOKEN` | PR and release access (provided automatically by Actions) |

You only need one of `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`, depending on which provider you use. For Claude models you can skip the stored API key entirely and use [workload identity federation](#anthropic-workload-identity-federation) instead.

## Basic: Update release on tag push

The simplest setup runs communiqué whenever you push a version tag:

```yaml
name: Release Notes

on:
  push:
    tags:
      - v[0-9]+.*

permissions:
  contents: write

jobs:
  release-notes:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique
      - name: Generate release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          # Use whichever provider you've configured:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          # OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: communique generate "${{ github.ref_name }}" --github-release
```

`fetch-depth: 0` is required so communiqué can read the full git history between tags.

## With GitHub Copilot

Use Copilot with the built-in Actions token in an organization-owned repository.
The organization must have Copilot access and enable **Allow use of Copilot CLI
billed to the organization**, as described in
[GitHub's authentication guide](https://github.com/github/copilot-sdk/blob/main/docs/auth/server-to-server-tokens.md#github-actions).
The permission is named `copilot-requests: write` (plural).

```yaml
name: Copilot Release Notes

on:
  push:
    tags:
      - v[0-9]+.*

permissions: {}

concurrency:
  group: copilot-release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  release-notes:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    permissions:
      contents: write
      pull-requests: read
      copilot-requests: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique --locked
      - name: Generate release notes with Copilot
        env:
          RELEASE_TAG: ${{ github.ref_name }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          OPENAI_API_KEY: ${{ secrets.GITHUB_TOKEN }}
          OPENAI_BASE_URL: https://api.githubcopilot.com
        run: |
          communique generate "$RELEASE_TAG" --github-release \
            --provider openai --model gpt-4.1 \
            --header 'copilot-integration-id: copilot-developer-cli'
```

This sends the Actions token and Copilot integration header to the
OpenAI-compatible Copilot endpoint, then publishes the generated release notes.
Choose a model available to your organization's Copilot plan.
Alternatively, replace `--header` with an environment variable:

```yaml
OPENAI_CUSTOM_HEADERS: '{"copilot-integration-id":"copilot-developer-cli"}'
```

## With release-plz

[release-plz](https://release-plz.ieni.dev/) automates versioning, changelogs, and crate publishing. communiqué can enhance the release notes it creates.

### Enhancing release notes

After release-plz creates a release, communiqué replaces the auto-generated notes with editorialized ones:

```yaml
name: Release-plz

on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run release-plz
        id: release-plz
        uses: release-plz/action@main
        with:
          command: release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
      - name: Fetch new tags
        if: steps.release-plz.outputs.releases_created == 'true'
        run: git fetch --tags
      - name: Enhance release notes
        if: steps.release-plz.outputs.releases_created == 'true'
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cargo install communique
          for tag in $(echo '${{ steps.release-plz.outputs.releases }}' | jq -r '.[].tag'); do
            communique generate "$tag" --github-release
          done
```

> **Note:** The `git fetch --tags` step is required because release-plz creates tags via the GitHub API, so they're not present in the local checkout.

### Release PRs and CHANGELOG.md

Let release-plz maintain the release PR description and `CHANGELOG.md` as commits
land. Run Communiqué in the `enhance-release` job above after release-plz creates
the release tag. This avoids paying to regenerate notes on every release PR
update and then generating them again for the published release.

If you want AI-generated changelog entries before merging, run
`communique generate <TAG> --changelog` explicitly when the release is ready for
review. This invokes the LLM and updates the changelog; it should not run on every
push to the release PR.

## Dry run in PRs

Run communiqué in `--dry-run` mode on pull requests to preview release notes without publishing. This still invokes the LLM and incurs API charges on each run:

```yaml
name: Preview Release Notes

on:
  pull_request:
    branches: [main]

jobs:
  preview:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique
      - name: Preview release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          TAG="v$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[0].version')"
          communique generate "$TAG" --dry-run
```

## Anthropic workload identity federation

Instead of storing a long-lived `ANTHROPIC_API_KEY` secret, a workflow can prove its identity to Anthropic with the OIDC token GitHub issues to each job. communiqué exchanges that token for an Anthropic access token that expires on its own, so there is no key to leak or rotate.

First, in the Claude Console, [connect a GitHub Actions workload](https://platform.claude.com/docs/en/manage-claude/wif-providers/github-actions): this registers GitHub as an issuer and creates a service account and a federation rule. Restrict the rule to the repository and branch or tag that publishes your releases.

Then grant the job `id-token: write` and set the IDs from the Console. They are identifiers, not secrets, so they can live in the workflow file or in repository variables:

```yaml
permissions:
  contents: write
  id-token: write # lets the job request a GitHub OIDC token

jobs:
  release-notes:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique
      - name: Generate release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_FEDERATION_RULE_ID: fdrl_...
          ANTHROPIC_ORGANIZATION_ID: 00000000-0000-0000-0000-000000000000
          ANTHROPIC_SERVICE_ACCOUNT_ID: svac_...
        run: communique generate "${{ github.ref_name }}" --github-release
```

No step is needed to fetch or exchange the token: when the federation variables are set and `ANTHROPIC_API_KEY` is not, communiqué requests the OIDC token from the runner (audience `https://api.anthropic.com`), exchanges it at `/v1/oauth/token`, and renews the access token if a run outlives it.

- `ANTHROPIC_API_KEY` takes precedence when it is set to a non-empty value, so remove it from the job when switching to federation.
- Set `ANTHROPIC_WORKSPACE_ID` (`wrkspc_...`) when the federation rule covers more than one workspace.
- Set `COMMUNIQUE_ANTHROPIC_IDENTITY_TOKEN_AUDIENCE` if your federation rule matches a different audience.
- Outside GitHub Actions, or to supply the identity token yourself, set `ANTHROPIC_IDENTITY_TOKEN_FILE` to a file holding the JWT (reread on every exchange) or `ANTHROPIC_IDENTITY_TOKEN` to the JWT itself. Either one takes precedence over the runner's token.
- If the exchange is denied, Anthropic returns only `Authentication failed`; the reason is shown on the [authentication history page](https://platform.claude.com/settings/workload-identity-federation?tab=history) in the Claude Console.

## Environment variables

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | API key for Anthropic (Claude models) |
| `ANTHROPIC_FEDERATION_RULE_ID` | Federation rule (`fdrl_...`) for [workload identity federation](#anthropic-workload-identity-federation) |
| `ANTHROPIC_ORGANIZATION_ID` | Anthropic organization UUID, required with the federation rule |
| `ANTHROPIC_SERVICE_ACCOUNT_ID` | Optional service account (`svac_...`) the federation rule targets |
| `ANTHROPIC_WORKSPACE_ID` | Workspace (`wrkspc_...`), required when the federation rule covers several |
| `ANTHROPIC_IDENTITY_TOKEN_FILE` | Path to an OIDC identity token to exchange, instead of the GitHub Actions one |
| `ANTHROPIC_IDENTITY_TOKEN` | OIDC identity token to exchange, as a literal value |
| `COMMUNIQUE_ANTHROPIC_IDENTITY_TOKEN_AUDIENCE` | Audience for the GitHub Actions OIDC token (default `https://api.anthropic.com`) |
| `OPENAI_API_KEY` | API key for OpenAI-compatible providers |
| `OPENAI_BASE_URL` | Custom OpenAI-compatible API endpoint |
| `OPENAI_CUSTOM_HEADERS` | JSON object of HTTP headers; repeatable `--header` flags take precedence |
| `GITHUB_TOKEN` | Token for GitHub API access (PR details, releases) |

The provider is auto-detected from the model name: `claude-*` models use Anthropic, everything else uses OpenAI-compatible endpoints.
