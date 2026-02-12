# `communique generate`

Synthesize editorialized release notes for a git tag.

## Usage

```
communique generate <TAG> [OPTIONS]
```

## Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<TAG>` | Yes | Target git tag |

## Options

| Flag | Description | Default |
|------|-------------|---------|
| `--prev_tag <TAG>` | Previous tag to diff against | Auto-detected |
| `--github-release` | Transmit notes to the GitHub Release | Off |
| `--concise` | Output concise changelog entry only | Off |
| `--repo <OWNER/REPO>` | GitHub repo | Auto-detected from remote |
| `--model <MODEL>` | Model identifier | `claude-opus-4-6` |
| `--max-tokens <N>` | Max response tokens | `4096` |

## Examples

Generate with automatic tag detection:

```sh
communique generate v1.2.0
```

Specify the previous tag explicitly:

```sh
communique generate v1.2.0 --prev_tag v1.1.0
```

Concise changelog entry only:

```sh
communique generate v1.2.0 --concise
```

Generate and publish to GitHub:

```sh
communique generate v1.2.0 --github-release
```
