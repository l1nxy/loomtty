# CLI Reference

The `loomtty` binary is both the client and the entry point for managing
sessions. The client auto-starts `loomtty-server` when needed.

## Synopsis

```bash
loomtty [SESSION]          # attach to / create a session
loomtty <COMMAND> [ARGS]   # run a subcommand
```

## Sessions

| Command | Description |
| ------- | ----------- |
| `loomtty` | Attach to the last session, or create one if none exists. |
| `loomtty <name>` | Attach to a session by name, creating it if needed. |
| `loomtty ls` | List sessions. |
| `loomtty a <name>` | Attach to an existing session. |
| `loomtty kill <name>` | Kill a session. |

## Other commands

| Command | Description |
| ------- | ----------- |
| `loomtty init` | Interactive config wizard (keybindings, leader, theme, status bar). |
| `loomtty web` | Serve the current session as a browser UI / PWA. See [Web UI](/guide/web-ui). |
| `loomtty --version` | Print the version. |

::: tip
`loomtty init` is an interactive TUI — it picks your keybinding style (prefix vs.
sticky), leader key, color theme, and status bar position. Shell and font are
auto-detected. Skip it and loomtty runs with defaults.
:::

## See also

- [Quick Start](/guide/quick-start) — the session workflow in practice
- [Keybindings](/guide/keybindings) — in-session keys
- [Configuration](/reference/configuration) — `config.toml` options
