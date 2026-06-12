# Layout & Workspaces

loomtty's layout is **column-based and scrollable**, inspired by
[niri](https://github.com/YaLTeR/niri). It's a little different from tmux's
grid, so it's worth a minute to understand the model — once it clicks, the
navigation keys make sense.

## The three levels

```
Workspace (scrolls vertically  — j / k)
└── Column (scrolls horizontally — h / l)
    └── Tile  (stacked panes in a column — shift+d)
```

- **Workspace** — a horizontal strip of columns. Workspaces are stacked
  **vertically**; you scroll **up/down** between them with `j` / `k`.
- **Column** — a full-height slot inside a workspace. Columns are laid out
  **horizontally** and scroll **left/right** with `h` / `l`. When there are more
  columns than fit, the viewport scrolls (it doesn't shrink everything to fit).
- **Tile** — a column can hold several panes stacked vertically. Each stacked
  pane is a **tile**.

::: tip Mnemonic
**h / l** move *across* columns (horizontal). **j / k** move *down / up* through
workspaces (vertical). Same as Vim directions, applied to the two scroll axes.
:::

## Creating things

| Action | Default key | What it does |
| ------ | ----------- | ------------ |
| New column | `Alt n` | Opens a new column to the right of the focused one. |
| New workspace | `Alt d` | Opens a new workspace below, with a fresh pane. |
| New stacked tile | `Alt Shift+d` | Stacks a new pane (tile) inside the current column. |
| Close pane | `Alt x` | Closes the focused pane. |

## Moving & reorganizing

| Action | Default key | What it does |
| ------ | ----------- | ------------ |
| Move column left / right | `Alt Shift+h` / `Alt Shift+l` | Reorder the focused column within its workspace. |
| Consume into column | `Alt c` | Pull the right-neighbor column's pane into the current column as a tile. |
| Expel from column | `Alt e` | Push the focused tile out into its own new column. |

`consume` and `expel` are how you turn a row of columns into a stack of tiles and
back — they're the loomtty equivalent of "join / break pane".

## Column widths

Columns don't auto-fill the screen; each has a width you control. Enter
**resize mode** with `Alt r`, then:

| Key (resize mode) | Action |
| ----------------- | ------ |
| `h` / `l` (or `[` / `]`) | Decrease / increase width |
| `r` / `Shift+r` | Cycle through preset widths (forward / reverse) |
| `f` | Full width |
| `=` | Equalize the column and its right neighbor |

The preset widths cycled by `r` default to **⅓, ½, ⅔, full** and are
configurable — see [`layout.preset_widths`](/reference/configuration#layout). How
wide a *newly opened* column is (and whether that adapts to window size) is
controlled by `layout.new_pane_sizing` / `new_pane_width`.

`Alt f` jumps the focused column straight to full width from anywhere (no resize
mode needed).

## Following the focus

As you move between columns, the viewport scrolls to keep the focused column in
view. How aggressively it centers is set by
[`layout.center_focused_column`](/reference/configuration#layout):

- `never` (default) — minimal scrolling; keep neighbors visible when possible.
- `on-overflow` — center only when the focused column and the previous one can't
  both fit (PaperWM-style).
- `always` — always center the focused column.

## Overview

Press `Alt o` for **overview** — a zoomed-out view of every workspace and column
at once, like a bird's-eye map. Navigate with `h` / `j` / `k` / `l`, open a new
column with `n`, close one with `x`, and press `o` / `Esc` / `Enter` / `Tab` to
drop back into the focused pane.

![Overview mode: every workspace zoomed out over the wallpaper](/demo/overview.png)

## Workspaces & sessions

- **Workspaces** live inside one session and scroll vertically (`j` / `k`).
- **Sessions** are separate, server-owned layouts you attach to. Switch the
  active connection with `Alt i` / `Alt Shift+i`, start a new session with
  `Alt Shift+n`, or open the **session palette** with `Alt Shift+p` to jump to
  any session or remote host by name.

See [Keybindings](/guide/keybindings) for the complete map and
[Remote & Predictive Echo](/guide/remote-attach) for attaching to sessions on
another host.
