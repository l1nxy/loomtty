# CLI Reference

The `loomtty` binary is both the client and the entry point for managing
sessions. The client auto-starts `loomtty-server` when needed.

## Synopsis

```bash
loomtty [SESSION]          # attach to / create a session
loomtty <COMMAND> [ARGS]   # run a subcommand
```

Running `loomtty` with no arguments attaches to the last session, or creates one
if none exists. A bare positional argument is treated as a session name.

## Sessions

| Command | Aliases | Description |
| ------- | ------- | ----------- |
| `loomtty` | | Attach to the last session, or create one if none exists. |
| `loomtty <name>` | | Attach to a session by name, creating it if needed. |
| `loomtty new` | | Create a new session and connect. |
| `loomtty attach <name>` | `a` | Attach to an existing session (errors if it doesn't exist). |
| `loomtty list` | `ls` | List sessions. |
| `loomtty kill <name>` | `k` | Kill a running session. |
| `loomtty delete <name>` | `rm` | Delete a saved (inactive) session. |
| `loomtty kill-server` | `ks` | Kill the server daemon. |

**`loomtty list` flags**

| Flag | Description |
| ---- | ----------- |
| `-a`, `--all` | Include saved (inactive) sessions, not just running ones. |

## Remote

Connect to a `loomtty-server` on another host over an SSH tunnel. See
[Remote & Predictive Echo](/guide/remote-attach) for the full setup.

```bash
loomtty remote <user@host> [session] [--port 7890] [--ssh-port 22]
```

| Argument / flag | Default | Description |
| --------------- | ------- | ----------- |
| `<user@host>` | — | SSH target (required). |
| `[session]` | — | Session to attach to / create on the remote. |
| `--port <n>` | `7890` | The remote's `[remote] port` (the loopback TCP port the server listens on). |
| `--ssh-port <n>` | `22` | SSH port for the tunnel. |

## Configuration

### `loomtty init`

Interactive setup wizard — generates a config file.

It's a TUI that walks you through keybinding style (prefix vs. sticky), leader
key, color theme, and status-bar position. Shell and font are auto-detected.
Skip it and loomtty runs with defaults. See [Configuration](/guide/configuration).

## Browser UI

### `loomtty web`

Serve the browser web UI (HTTP + WebSocket) and run the server in the
**foreground**. On first run it enables `[web]`, mints a token, and persists both
to your config so the desktop app shares them. See [Web UI](/guide/web-ui).

```bash
loomtty web [--port <n>] [--bind <addr>] [--token <str>] [--static-dir <path>] [--open]
```

| Flag | Default | Description |
| ---- | ------- | ----------- |
| `--port <n>` | `web.port`, else `7891` | Listen port. |
| `--bind <addr>` | `127.0.0.1` | Bind address. Use `0.0.0.0` to expose on the LAN — then also set `web.allowed_origins`. |
| `--token <str>` | reuse configured, else generate | Auth token (32 hex chars when generated); saved to config. |
| `--static-dir <path>` | `<server-dir>/web` | Directory of the built web bundle. |
| `--open` | off | Open the URL in your default browser after starting. |

::: warning Token on shared hosts
A `--token` value is visible to other users in process listings. On a shared
host, prefer the `LOOMTTY_WEB_TOKEN` environment variable instead.
:::

## Layout templates

### `loomtty template` (`tpl`)

Save and reapply pane layouts.

```bash
loomtty template <SUBCOMMAND>
```

| Subcommand | Aliases | Arguments | Description |
| ---------- | ------- | --------- | ----------- |
| `list` | `ls` | | List layout templates. |
| `apply` | | `<template> [session]` | Apply a template (to the given or current session). |
| `save` | | `<template> <session>` | Save a session's layout as a template. |

## Scripting (IPC)

### `loomtty msg`

Drive a session from the shell — useful for scripting and automation. Add
`--json` to any subcommand for machine-readable output.

```bash
loomtty msg <SUBCOMMAND> [--json]
```

| Subcommand | Arguments | Description |
| ---------- | --------- | ----------- |
| `send-keys` | `<session> <pane-id> <keys>` | Send keystrokes to a pane. |
| `list-panes` | `<session>` | List all panes in a session. |
| `info` | `<session>` | Get session info. |
| `focus-pane` | `<session> <pane-id>` | Focus a pane by ID. |
| `close-pane` | `<session> <pane-id>` | Close a pane by ID. |
| `create-pane` | `<session>` | Create a new pane. |
| `get-layout` | `<session>` | Get the full layout state. |
| `run-command` | `<session> <command>` | Run a command in a new pane. |
| `capture-pane` | `<session> <pane-id>` | Capture a pane's text to stdout (see flags below). |
| `list-prompts` | `<session> <pane-id>` | List recorded OSC 133 prompt boundaries. |

**`capture-pane` flags**

| Flag | Default | Description |
| ---- | ------- | ----------- |
| `--scrollback-rows <n>` | `0` | Rows of scrollback above the viewport to include (`0` = viewport only). Clamped to the pane's history, a 100k row cap, and a ~900 KiB byte budget. |
| `--join-wrapped` | off | Join soft-wrapped lines (omit the newline between wrapped rows), like tmux `capture-pane -J`. |
| `--preserve-trailing-spaces` | off | Keep trailing ASCII-space cells on each row (default trims them). |

::: tip capture-pane and alt-screen apps
When an alt-screen TUI (vim, less, htop) is foregrounded, the alt buffer is
captured and `--scrollback-rows` has no effect — the primary buffer's history
isn't reachable. `list-prompts` returning nothing means shell integration isn't
active in that pane.
:::

## The server daemon

`loomtty-server` is normally **auto-started** by the client — you rarely run it
by hand. When you do, these flags apply:

| Flag | Description |
| ---- | ----------- |
| _(none)_ | Run with the [system tray](/guide/system-tray) (default; needs a desktop session). |
| `--headless` | Run in the foreground without the tray — for headless / SSH-only hosts. |
| `--daemonize` | Unix only: fork into the background (also no tray). |
| `--print-socket-path` | Print the IPC socket path and exit. |
| `--web` `--web-port <n>` `--web-bind <addr>` `--web-token <tok>` `--web-static-dir <dir>` | Enable and configure the [web gateway](/guide/web-ui) (what `loomtty web` passes through). |

## Global

| Flag | Description |
| ---- | ----------- |
| `--version` | Print the version. |
| `--help`, `-h` | Print help (works per subcommand too, e.g. `loomtty msg capture-pane --help`). |

## See also

- [Quick Start](/guide/quick-start) — the session workflow in practice
- [Keybindings](/guide/keybindings) — in-session keys
- [Configuration](/reference/configuration) — `config.toml` options
- [Remote & Predictive Echo](/guide/remote-attach) — remote attach setup
