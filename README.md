# loomtty

GPU-accelerated terminal multiplexer with column-based layouts, workspaces, and remote support.

## Quick Start

```bash
# Launch (auto-attach to last session or create new)
loomtty

# Named session
loomtty my-project

# List sessions
loomtty ls

# Attach to existing session
loomtty a my-project
```

## Keybindings

Leader key: `Ctrl+W` (configurable)

### Navigation

| Key | Action |
|-----|--------|
| `Leader` `h` / `Left` | Focus left |
| `Leader` `l` / `Right` | Focus right |
| `Leader` `k` / `Up` | Focus up |
| `Leader` `j` / `Down` | Focus down |

### Pane Management

| Key | Action |
|-----|--------|
| `Leader` `n` | New column |
| `Leader` `d` | New workspace (split down) |
| `Leader` `x` | Close pane |
| `Leader` `Shift+H` | Move pane left |
| `Leader` `Shift+L` | Move pane right |
| `Leader` `c` | Consume right neighbor into column |
| `Leader` `e` | Expel pane to new column |

### Column Width

| Key | Action |
|-----|--------|
| `Leader` `f` | Full width |
| `Leader` `r` | Enter resize mode |

Resize mode (`Leader` `r`):

| Key | Action |
|-----|--------|
| `h` / `[` | Decrease width |
| `l` / `]` | Increase width |
| `r` | Cycle preset (1/3, 1/2, 2/3, full) |
| `=` | Equalize columns |
| `Escape` | Exit resize mode |

### Modes

| Key | Action |
|-----|--------|
| `Leader` `s` | Scroll mode |
| `Leader` `m` | Move mode |
| `Leader` `b` | Toggle broadcast (input to all panes) |
| `Ctrl+G` | Toggle lock (passthrough all keys) |
| `Leader` `Tab` | Toggle overview |
| `Leader` `p` | Command palette |
| `Leader` `q` | Detach |

### Scroll Mode

| Key | Action |
|-----|--------|
| `j` / `k` | Line down / up |
| `d` / `u` | Half page down / up |
| `f` / `b` | Page down / up |
| `g` / `G` | Top / bottom |

### Direct Bindings (no leader)

| Key | Action |
|-----|--------|
| `Ctrl+Shift+F` | Search |
| `Ctrl+Shift+C` | Copy |
| `Ctrl+Shift+V` | Paste |

## CLI

```bash
# Session management
loomtty new                        # New session
loomtty ls [-a]                    # List sessions (--all includes saved)
loomtty a SESSION                  # Attach to session
loomtty kill SESSION               # Kill session
loomtty kill-server                # Kill server daemon
loomtty rm SESSION                 # Delete saved session state

# Remote
loomtty remote HOST [SESSION] [--port PORT] [--ssh-port SSH_PORT]

# Web UI (browser terminal) — serve the SPA + WS gateway, print the URL+token
loomtty web [--port PORT] [--bind ADDR] [--token TOK] [--static-dir DIR] [--open]

# IPC scripting
loomtty msg send-keys SESSION PANE_ID KEYS
loomtty msg list-panes SESSION [--json]
loomtty msg run-command SESSION COMMAND
loomtty msg info SESSION
loomtty msg focus-pane SESSION PANE_ID
loomtty msg close-pane SESSION PANE_ID
loomtty msg create-pane SESSION
loomtty msg get-layout SESSION
loomtty msg capture-pane SESSION PANE_ID            # active grid to stdout
  [--scrollback-rows N]                             #   include N rows of scrollback (clamped server-side)
  [--join-wrapped]                                  #   merge soft-wrapped rows (drops the \n between them)
  [--preserve-trailing-spaces]                      #   keep trailing ASCII spaces on each row
  [--json]                                          #   wrap in JSON {session_name, pane_id, text, truncated}
# • Captures the *active* buffer: when an alt-screen TUI (vim/less/htop)
#   is foregrounded, you'll get its buffer and `--scrollback-rows` has
#   no effect (the primary buffer's history is unreachable).
# • Output is capped at ~900 KiB to fit a single control frame; very
#   wide panes with deep scrollback get fewer rows than requested.
loomtty msg list-prompts SESSION PANE_ID [--json]   # OSC 133 shell-integration history
# • One entry per command boundary: prompt_line, output_line, done_line
#   (absolute-line numbers; monotonic), exit_code, duration_ms.
# • Empty when the pane hasn't observed OSC 133 — i.e. shell integration
#   isn't active in the foreground program.
# • Default text format prints a fixed-width table; --json emits a JSON
#   array suitable for jq / scripting.

# Templates
loomtty tpl ls                     # List templates
loomtty tpl save NAME SESSION      # Save layout as template
loomtty tpl apply NAME [SESSION]   # Apply template
```

## Configuration

Config file: `~/.config/loom/config.toml`

Run `loomtty init` for interactive setup.

```toml
[font]
family = "JetBrains Mono"
size = 12.0

[theme]
preset = "loom_dark"
# Available: loom_dark, one_dark, one_half_dark, catppuccin_mocha,
#            tokyo_night, dracula, nord, gruvbox_dark, ghostty

[terminal]
shell = ""                    # Empty = platform default
scrollback_lines = 10000
cursor_blink = true
copy_on_select = false

[appearance]
column_gap = 8.0
padding = 4.0
inactive_opacity = 0.7
# 0.0 keeps classic sharp panes; try 4-12 for modern soft corners.
# Rounds pane backgrounds, cell content, and focus rings uniformly.
pane_corner_radius = 0.0     # 0.0 = sharp; try 4-12 for soft panes

[input]
mode = "prefix"               # "prefix" (tmux-style) or "sticky" (zellij-style)
leader_timeout_ms = 1000
focus_follows_mouse = false

[keys]
leader = "ctrl+w"

[layout]
default_column_width = { proportion = 0.5 }
center_focused_column = "always"  # "always", "on-overflow", "never"

[animation]
enabled = true
preset = "default"            # "snappy", "default", "smooth", "gentle"

[statusbar]
position = "top"              # "top" or "bottom"

[prediction]
mode = "never"                # "never", "always", "adaptive" (mosh-style)

[remote]
enabled = false
port = 7890

[[remote.hosts]]
name = "my-server"
host = "user@example.com"
port = 7890
ssh_port = 22

[web]                         # browser terminal (HTTP SPA + /ws gateway)
enabled = false
bind = ""                     # empty = 127.0.0.1
port = 7891
token = ""                    # ≥16 bytes; openssl rand -hex 16
allowed_origins = []          # required (exact-match) for non-loopback binds
static_dir = ""               # empty = <server-exe-dir>/web
```

### Web UI

`loomtty web` turns the desktop multiplexer into a browser terminal: it
enables `[web]`, mints and saves a token, prints the access URL, and runs
the server. Build the SPA into the server's static dir once with
`node web/scripts/install-assets.mjs`, then open the printed URL and log in
with the token. The gateway speaks plain `http`/`ws` — front it with TLS
(reverse proxy or tunnel) before exposing it past loopback. See
[`web/README.md`](web/README.md) for the full guide.
