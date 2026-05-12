# ciritty

GPU-accelerated terminal multiplexer with column-based layouts, workspaces, and remote support.

## Quick Start

```bash
# Launch (auto-attach to last session or create new)
ciritty

# Named session
ciritty my-project

# List sessions
ciritty ls

# Attach to existing session
ciritty a my-project
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
ciritty new                        # New session
ciritty ls [-a]                    # List sessions (--all includes saved)
ciritty a SESSION                  # Attach to session
ciritty kill SESSION               # Kill session
ciritty kill-server                # Kill server daemon
ciritty rm SESSION                 # Delete saved session state

# Remote
ciritty remote HOST [SESSION] [--port PORT] [--ssh-port SSH_PORT]

# IPC scripting
ciritty msg send-keys SESSION PANE_ID KEYS
ciritty msg list-panes SESSION [--json]
ciritty msg run-command SESSION COMMAND
ciritty msg info SESSION
ciritty msg focus-pane SESSION PANE_ID
ciritty msg close-pane SESSION PANE_ID
ciritty msg create-pane SESSION
ciritty msg get-layout SESSION
ciritty msg capture-pane SESSION PANE_ID            # active grid to stdout
  [--scrollback-rows N]                             #   include N rows of scrollback (clamped server-side)
  [--join-wrapped]                                  #   merge soft-wrapped rows (drops the \n between them)
  [--preserve-trailing-spaces]                      #   keep trailing ASCII spaces on each row
  [--json]                                          #   wrap in JSON {session_name, pane_id, text, truncated}
# • Captures the *active* buffer: when an alt-screen TUI (vim/less/htop)
#   is foregrounded, you'll get its buffer and `--scrollback-rows` has
#   no effect (the primary buffer's history is unreachable).
# • Output is capped at ~900 KiB to fit a single control frame; very
#   wide panes with deep scrollback get fewer rows than requested.
ciritty msg list-prompts SESSION PANE_ID [--json]   # OSC 133 shell-integration history
# • One entry per command boundary: prompt_line, output_line, done_line
#   (absolute-line numbers; monotonic), exit_code, duration_ms.
# • Empty when the pane hasn't observed OSC 133 — i.e. shell integration
#   isn't active in the foreground program.
# • Default text format prints a fixed-width table; --json emits a JSON
#   array suitable for jq / scripting.

# Templates
ciritty tpl ls                     # List templates
ciritty tpl save NAME SESSION      # Save layout as template
ciritty tpl apply NAME [SESSION]   # Apply template
```

## Configuration

Config file: `~/.config/ciri/config.toml`

Run `ciritty init` for interactive setup.

```toml
[font]
family = "JetBrains Mono"
size = 12.0

[theme]
preset = "ciri_dark"
# Available: ciri_dark, one_dark, one_half_dark, catppuccin_mocha,
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
```
