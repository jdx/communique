# `communique init`

Generate a `communique.toml` manifest in the repository root.

## Usage

```
communique init [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--force` | Overwrite existing manifest |

## Examples

Initialize a new workspace:

```sh
communique init
```

Reinitialize, overwriting existing configuration:

```sh
communique init --force
```
