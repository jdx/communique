# CLI Reference

The communiqué command interface provides two operations:

| Command | Description |
|---------|-------------|
| [`generate`](./generate) | Synthesize release notes for a git tag |
| [`init`](./init) | Create a `communique.toml` manifest |

## Usage

```
communique <COMMAND> [OPTIONS]
```

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `ANTHROPIC_API_KEY` | Yes | Credentials for neural network access |
| `GITHUB_TOKEN` | For GitHub features | GitHub personal access token for uplink |
