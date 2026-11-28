# GitLab

communiqué works with projects on GitLab.com and on self-managed GitLab
instances. It reads merge requests, merge request diffs, and issues through the
GitLab REST API, and it can create or update the GitLab release for a tag.

## Detection

communiqué decides which forge hosts the project in this order:

1. `forge` (or `forge_url`) under `[defaults]` in `communique.toml`.
2. GitLab CI's predefined variables (`GITLAB_CI`, `CI_SERVER_URL`, and
   `CI_PROJECT_PATH`), so CI jobs on any GitLab instance need no configuration.
3. The host of the `origin` remote: `github.com` or `gitlab.com`.

For a self-managed instance outside GitLab CI, set the forge and its URL:

```toml
[defaults]
forge = "gitlab"
forge_url = "https://gitlab.example.com"
# repo = "group/subgroup/project"  # optional; read from the origin remote
```

Nested group paths such as `group/subgroup/project` are supported. A `repo`
setting without `forge` still means a GitHub `owner/repo`.

## Token

Set `GITLAB_TOKEN` to a personal, group, or project access token with the `api`
scope. Reading merge requests needs at least read access; creating or updating
releases needs a role that can manage releases (Developer or higher).

```sh
export GITLAB_TOKEN="glpat-..."
communique generate v1.2.0 --gitlab-release
```

`--gitlab-release` updates the release for the tag, or creates it when the tag
has no release yet. Without a token, generation still works from the local git
history, but merge request details are unavailable to the model.

communiqué does not use the CI job token (`CI_JOB_TOKEN`). Job tokens have
[restricted API access](https://docs.gitlab.com/ci/jobs/ci_job_token/) that
varies by GitLab version and instance settings, and merge request details are
often unavailable through them.

## Merge requests

Commits are matched to merge requests from `!123` and
`See merge request group/project!123` references in commit messages. Squash and
fast-forward merges often leave no reference, so for those commits communiqué
asks GitLab which merged merge request contains them (up to 100 commits per
release). Label rules in `[rules]` apply to merge request labels.

Generated notes link merge requests, issues, commits, and comparisons to the
project's GitLab URL, for example `https://gitlab.example.com/group/project/-/merge_requests/123`.

## GitLab CI

Store `GITLAB_TOKEN` and your LLM key (for example `ANTHROPIC_API_KEY`) as
masked CI/CD variables, then run communiqué on tag pipelines:

```yaml
release-notes:
  image: rust:latest
  rules:
    - if: $CI_COMMIT_TAG
  variables:
    GIT_DEPTH: "0" # full history so communiqué can compare tags
  script:
    - cargo install communique
    - communique generate "$CI_COMMIT_TAG" --gitlab-release
```

`GIT_DEPTH: "0"` disables GitLab's default shallow clone; communiqué needs the
history between the previous tag and the new one.

## Drafts

`generate --draft` records the forge in the JSON draft, and `publish` uses
`GITLAB_TOKEN` to create or update the GitLab release from it. Drafts for GitHub
projects keep their existing format.

A draft for a self-managed instance names that instance's URL. Before sending
`GITLAB_TOKEN`, `publish` checks that the URL matches `forge_url` in
`communique.toml`, `CI_SERVER_URL` in GitLab CI, or the host of the `origin`
remote, and refuses to publish otherwise.
