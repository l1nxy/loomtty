# Quick Start

Once `loomtty` and `loomtty-server` are on your `PATH`, you're ready. The client
auto-starts the server on first launch.

## Sessions

A **session** is a set of panes and layout owned by the server. It outlives the
client, so you can detach and reattach — even from a different machine.

```bash
loomtty                  # attach to the last session, or create one
loomtty my-project       # attach to / create a named session
loomtty ls               # list sessions
loomtty a my-project     # attach to an existing session
loomtty kill my-project  # kill a session
```

See the [CLI reference](/reference/cli) for the full command list.

## The first session

1. Run `loomtty`. You get a single pane running your shell.
2. Hold the leader (`Alt` by default) and press `n` to open a **new column**.
3. `Alt h` / `l` move focus between **columns**; `Alt j` / `k` move between
   **workspaces** (stacked vertically).
4. `Alt Shift+d` stacks a **tile** inside the current column; `Alt d` opens a new
   **workspace** below.
5. `Alt q` **detaches** — your shells keep running in the server. Run `loomtty`
   again to reattach right where you left off.

::: tip Leader key
loomtty defaults to **sticky / zellij-style** input: you *hold* `Alt` and press
the key. Prefer **tmux-style** tap-leader-then-key? Set `[input] mode = "prefix"`
in your config. See [Keybindings](/guide/keybindings).
:::

## Layout model

loomtty arranges panes in **scrollable columns** (niri-style): workspaces stack
vertically, columns sit side by side and scroll horizontally, and a column can
hold several stacked **tiles**. It's worth a quick read —
see [Layout & Workspaces](/guide/layout).

| Want to… | Press |
| -------- | ----- |
| New column | `Alt n` |
| New workspace below | `Alt d` |
| Stack a tile in the column | `Alt Shift+d` |
| Make the focused column full-width | `Alt f` |
| See everything at once | `Alt o` (overview) |
| Run any command by name | `Alt p` (command palette) |

## Configure it

Run `loomtty init` for the interactive wizard, or edit
`~/.config/loom/config.toml` directly — see [Configuration](/guide/configuration).
You can also change most settings live from the in-app settings panel.

## Where next

- [Layout & Workspaces](/guide/layout) — the column / workspace / tile model
- [Keybindings](/guide/keybindings) — the full default map
- [Configuration](/guide/configuration) — fonts, themes, input modes
- [Theming](/guide/theming) — presets and color overrides
- [Shell Integration](/guide/shell-integration) — prompt marks and jump-to-prompt
- [Remote & Predictive Echo](/guide/remote-attach) — sessions on another host
- [Web UI](/guide/web-ui) — open your session in a browser
