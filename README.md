# ciri

GPU-accelerated terminal multiplexer with column-based layouts, workspaces, and remote support.

## Quick Start

```bash
# Launch (auto-attach to last session or create new)
ciri

# Named session
ciri my-project

# List sessions
ciri ls

# Attach to existing session
ciri a my-project
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
ciri new                           # New session
ciri ls [-a]                       # List sessions (--all includes saved)
ciri a SESSION                     # Attach to session
ciri kill SESSION                  # Kill session
ciri kill-server                   # Kill server daemon
ciri rm SESSION                    # Delete saved session state

# Remote
ciri remote HOST [SESSION] [--port PORT] [--ssh-port SSH_PORT]

# IPC scripting
ciri msg send-keys SESSION PANE_ID KEYS
ciri msg list-panes SESSION [--json]
ciri msg run-command SESSION COMMAND
ciri msg info SESSION
ciri msg focus-pane SESSION PANE_ID
ciri msg close-pane SESSION PANE_ID
ciri msg create-pane SESSION
ciri msg get-layout SESSION

# Templates
ciri tpl ls                        # List templates
ciri tpl save NAME SESSION         # Save layout as template
ciri tpl apply NAME [SESSION]      # Apply template
```

## Configuration

Config file: `~/.config/ciri/config.toml`

Run `ciri init` for interactive setup.

```toml
[font]
family = "JetBrains Mono"
size = 12.0

[theme]
preset = "ciri_dark"
# Available: ciri_dark, one_dark, catppuccin_mocha,
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
```

