---
layout: home
title: Generate release notes from your repository
description: Use an AI model to draft release notes from commits, pull requests, and source changes. Review locally, update a changelog, or publish to GitHub Releases.

hero:
  name: communiqué
  text: Generate release notes from your repository
  image:
    src: /logo.svg
    alt: communiqué
  tagline: Communiqué uses an AI model to read commits, pull requests, and source changes, then draft release notes. Review the output locally, write a changelog entry, or publish to GitHub Releases.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/jdx/communique
---

<div class="release-workflow">

## Generate a draft

Run `communique init` to create a configuration file, then generate notes for a
release tag. Communiqué compares it with the previous tag and lets the model
inspect repository files and diffs for context. GitHub access adds pull request
details to that context.

```sh
communique generate v1.2.0 --draft release.json
```

You need an API key for Anthropic or an OpenAI-compatible provider. Configure the
model, project context, and writing style in `communique.toml`.

[Set up Communiqué](/guide/getting-started)

## Review before publishing

The saved draft is editable. Preview your changes locally, then publish the
reviewed draft without invoking the model again.

```sh
communique publish release.json --dry-run
communique publish release.json
```

Generation can also produce a coverage report and migration guide to help you
review what changed and what users need to do when upgrading.

[Configure review and output options](/guide/configuration)

## Update a changelog or GitHub Release

Use `--changelog` when generating to write a version entry to `CHANGELOG.md`, or
`--github-release` to publish directly to a GitHub Release. Add `--concise` when
you want a shorter summary. You can run these commands locally or in a release
workflow.

[Set up GitHub Actions](/guide/github-actions)

</div>
