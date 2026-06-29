# Keybindings

This page lists the **default keymap** as shipped in
[`config/default.toml`](https://github.com/l1nxy/loomtty/blob/main/config/default.toml).
Every binding is configurable — see [Customizing](#customizing) below and the
full [Actions reference](/reference/actions) for the complete set of bindable
actions.

## The leader key

Most actions go through a **leader key** — `Alt` by default
([`keys.leader`](/reference/configuration#keys)). How the leader behaves depends
on the input **mode** ([`input.mode`](/reference/configuration#input)):

- **`sticky`** (default, zellij-style) — **hold** the leader and press keys; you
  stay in leader mode for repeatable actions (focus, resize) until you release or
  press `Esc`.
- **`prefix`** (tmux-style) — **tap** the leader once, release, then press the
  key. One action, then back to normal.

```toml
[keys]
leader = "alt"

[input]
mode = "sticky"          # or "prefix"
leader_timeout_ms = 1000 # prefix mode: how long the leader stays armed
```

Throughout this page, `Alt x` means "leader + x".

## Normal bindings (with leader)

| Keys | Action | Description |
| ---- | ------ | ----------- |
| `Alt n` | `new_column_right` | New column to the right |
| `Alt d` | `new_row_below` | New workspace below (with a pane) |
| `Alt Shift+d` | `new_tile_below` | New stacked tile in the column |
| `Alt x` | `close_pane` | Close the focused pane |
| `Alt h` / `Alt l` | `focus_left` / `focus_right` | Focus column left / right |
| `Alt j` / `Alt k` | `focus_down` / `focus_up` | Focus workspace down / up |
| `Alt ←` / `→` / `↓` / `↑` | focus | Arrow-key mirror of `h` / `l` / `j` / `k` |
| `Alt Shift+h` / `Alt Shift+l` | `move_pane_left` / `move_pane_right` | Move column within the workspace |
| `Alt Shift+k` / `Alt Shift+j` | `move_pane_up` / `move_pane_down` | Move the pane to the workspace above / below (creates one at the edge) |
| `Alt r` | `enter_mode:resize` | Enter [resize mode](#resize-mode) |
| `Alt s` | `enter_mode:scroll` | Enter [scroll mode](#scroll-mode) |
| `Alt m` | `enter_mode:move` | Enter [move mode](#move-mode) |
| `Alt f` | `column_width_full` | Full-width column |
| `Alt b` | `toggle_broadcast` | Broadcast input to all panes in the workspace |
| `Alt c` | `consume_into_column` | Consume right neighbor into the column as a tile |
| `Alt e` | `expel_from_column` | Expel the active tile into a new column |
| `Alt o` | `toggle_overview` | Toggle [overview](#overview-mode) |
| `Alt q` | `detach` | Detach (server keeps running) |
| `Alt p` | `toggle_command_palette` | Command palette |
| `Alt Shift+p` | `toggle_session_palette` | Session / remote-host palette |
| `Alt i` / `Alt Shift+i` | `next_session` / `prev_session` | Switch connection |
| `Alt Shift+n` | `new_session` | New session |
| `Alt g` | `toggle_lock` | Lock (pass all keys through to the terminal) |
| `Alt /` | `toggle_help` | Keybindings help overlay |

## Direct bindings (no leader)

These work at any time, without the leader:

| Keys | Action |
| ---- | ------ |
| `Ctrl+Shift+F` | Open search |
| `Ctrl+Shift+C` | Copy selection |
| `Ctrl+Shift+V` | Paste |
| `Ctrl+G` | Toggle lock |
| `Alt+Shift+H` / `Alt+Shift+L` | Move the pane left / right within the workspace |
| `Alt+Shift+K` / `Alt+Shift+J` | Move the pane to the workspace above / below (creates one at the edge) |

::: tip macOS
On macOS the clipboard/search bindings use `Cmd` instead of `Ctrl+Shift`:
`Cmd+F` search, `Cmd+C` copy, `Cmd+V` paste, plus `Cmd+N` new pane, `Cmd+W`
close pane, and `Cmd+Q` quit.
:::

## Modes

Modes are temporary key tables you enter from the leader. In **sticky** input,
you stay in the mode until you press `Esc`.

### Resize mode

Enter with `Alt r`.

| Key | Action |
| --- | ------ |
| `h` / `l` | Decrease / increase column width |
| `[` / `]` | Decrease / increase column width |
| `r` / `Shift+r` | Cycle preset width (forward / reverse) |
| `f` | Full width |
| `=` | Equalize the column and its right neighbor |

::: tip
The actions `tile_height_increase` / `tile_height_decrease` adjust the height of
stacked tiles. Bind them in `[keys.modes.resize]` (e.g. `j` / `k`) if you use
tiles heavily — see the [Actions reference](/reference/actions).
:::

### Scroll mode

Enter with `Alt s`. Scrolls the focused pane's scrollback.

| Key | Action |
| --- | ------ |
| `j` / `k` (or `↓` / `↑`) | Line down / up |
| `d` / `u` | Half page down / up |
| `f` / `b` | Page down / up |
| `g` / `Shift+g` | Top / bottom |
| `[` / `]` | Previous / next prompt (needs [shell integration](/guide/shell-integration)) |

### Move mode

Enter with `Alt m`. Reposition the focused pane.

| Key | Action |
| --- | ------ |
| `h` / `l` (or `←` / `→`) | Move column left / right within the workspace |
| `k` / `j` (or `↑` / `↓`) | Move the pane to the workspace above / below (creates one at the edge) |

## Overview mode

Enter with `Alt o`.

| Key | Action |
| --- | ------ |
| `h` / `j` / `k` / `l` | Focus left / down / up / right |
| `n` | New column |
| `x` | Close pane |
| `o` / `Esc` / `Enter` / `Tab` | Exit overview |

## Overlay control keys

When the **search bar**, **command palette**, or **paste confirmation** dialog is
open, these keys apply:

::: code-group

```toml [Search]
[keys.search_bindings]
escape = "close_search"
enter = "search_next_match"
"shift+enter" = "search_prev_match"
backspace = "text_backspace"
```

```toml [Palette]
[keys.palette_bindings]
escape = "close_command_palette"
up = "palette_up"
down = "palette_down"
enter = "palette_confirm"
backspace = "text_backspace"
```

```toml [Paste confirm]
[keys.paste_confirm_bindings]
enter = "confirm_paste"
y = "confirm_paste"
escape = "dismiss_paste_confirm"
n = "dismiss_paste_confirm"
```

:::

## Key syntax

- **Modifiers**: `ctrl+`, `shift+`, `alt+`, `super+` (the macOS `Cmd` key),
  combined with `+` — e.g. `"ctrl+shift+f"`.
- **Named keys**: `Left`, `Right`, `Up`, `Down`, `escape`, `enter`, `tab`,
  `backspace` (case-insensitive).
- **Characters**: the literal character, e.g. `n`, `/`, `[`.

Actions are referenced by their string name (`"focus_left"`). Some take an
argument: `"enter_mode:resize"`, `"switch_workspace_3"`,
`"activate_key_table:mytable"`.

## Customizing

Bindings live under `[keys]` in `~/.config/loom/config.toml`:

```toml
[keys]
leader = "ctrl+a"

[keys.bindings]
n = "new_column_right"
v = "expel_from_column"
# …
```

::: warning Maps are replaced, not merged
Defining a `[keys.bindings]` table in your config **replaces the entire default
map** — the omitted defaults are *not* kept. The same applies to
`[keys.modes.*]`, `[keys.overview_bindings]`, and the overlay tables. If you want
the defaults plus a few changes, copy the whole block from
[`config/default.toml`](https://github.com/l1nxy/loomtty/blob/main/config/default.toml)
and edit it.
:::

See the [Actions reference](/reference/actions) for every action you can bind,
including aliases and the `switch_workspace_N` / `enter_mode:NAME` dynamic forms.
