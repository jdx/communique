# Configuration

communiqué is configured via a `communique.toml` manifest in your repository root. Generate one with:

```sh
communique init
```

## Directives

### `system_extra`

Additional instructions injected into the system prompt. Use this to shape the tone, style, or conventions of your output.

```toml
system_extra = """
Write in a casual, friendly tone.
Always mention breaking changes prominently.
"""
```

### `context`

Supplemental context included in every generation request. Useful for project descriptions or domain knowledge the agent should always have access to.

```toml
context = """
This is a Rust CLI tool for managing cloud infrastructure.
Our users are DevOps engineers and SREs.
"""
```

### `[defaults]`

Default parameters for generation. All values can be overridden via CLI flags.

```toml
[defaults]
model = "claude-opus-4-6"
max_tokens = 4096
repo = "owner/repo"
```

| Key | Description | Default |
|-----|-------------|---------|
| `model` | Model identifier | `claude-opus-4-6` |
| `max_tokens` | Maximum response tokens | `4096` |
| `repo` | GitHub repo in `owner/repo` format | Auto-detected from git remote |

## Resolution Order

Configuration is resolved with the following precedence:

1. CLI flags (highest priority)
2. `communique.toml` defaults
3. Built-in defaults
