# Configuration

loomtty reads its config from a TOML file. Run the interactive wizard with
`loomtty init`, edit the file by hand, or change most settings live from the
**in-app settings panel**.

## File location

```
~/.config/loom/config.toml
```

## A minimal example

```toml
[font]
family = "JetBrains Mono"
size = 12.0

[theme]
preset = "loom_dark"   # see the list below

[keys]
leader = "alt"

[input]
mode = "sticky"        # "sticky" (zellij-style) or "prefix" (tmux-style)
```

## Themes

Set `theme.preset` to one of the built-in presets:

`loom_dark` · `one_dark` · `one_half_dark` · `catppuccin_mocha` ·
`tokyo_night` · `dracula` · `nord` · `gruvbox_dark` · `ghostty`

```toml
[theme]
preset = "catppuccin_mocha"
```

## Input & keybindings

```toml
[keys]
leader = "alt"         # leader key

[input]
mode = "sticky"        # "sticky" (zellij-style) or "prefix" (tmux-style)
```

See [Keybindings](/guide/keybindings) for the full default map and how the two
input modes differ.

## All sections

`config.toml` is split into these sections. Every option, default, and valid
range is in the [**Configuration reference**](/reference/configuration):

| Section | What it covers |
| ------- | -------------- |
| [`[font]`](/reference/configuration#font) | Family, size, OpenType features, cell/underline metrics |
| [`[appearance]`](/reference/configuration#appearance) | Padding, borders, focus ring, background image, opacity |
| [`[animation]`](/reference/configuration#animation) | Enable/disable, speed preset, pane-open style |
| [`[window]`](/reference/configuration#window) | Initial size and title |
| [`[terminal]`](/reference/configuration#terminal) | Cursor, shell, scrollback, selection, bell |
| [`[statusbar]`](/reference/configuration#statusbar) | Status bar position and spacing |
| [`[tabbar]`](/reference/configuration#tabbar) | Pane-tab placement and sizing |
| [`[input]`](/reference/configuration#input) | Input mode, leader timing, scroll, focus-follows-mouse |
| [`[render]`](/reference/configuration#render) | GPU backend, present mode, alpha blending |
| [`[layout]`](/reference/configuration#layout) | Column widths, new-pane sizing, centering |
| [`[gesture]`](/reference/configuration#gesture) | Touchpad gestures, smooth scroll |
| [`[remote]`](/reference/configuration#remote) | Remote attach listener and saved hosts |
| [`[session]`](/reference/configuration#session) | Agent restore on reattach |
| [`[server]`](/reference/configuration#server) | Daemon idle timeout |
| [`[prediction]`](/reference/configuration#prediction) | Mosh-style predictive echo |
| [`[web]`](/reference/configuration#web) | Browser gateway |
| [`[keys]`](/guide/keybindings) | Leader and keybindings |
| [`[theme]`](/guide/theming) | Colors and presets |

The schema is the ultimate source of truth:
[`crates/loom-config/src/schema.rs`](https://github.com/l1nxy/loomtty/blob/main/crates/loom-config/src/schema.rs).

::: warning Keybinding & theme maps replace, not merge
Defining `[keys.bindings]` (or any other `[keys.*]` table) in your config
**replaces the whole default map**. To keep the defaults plus a few changes, copy
the block from
[`config/default.toml`](https://github.com/l1nxy/loomtty/blob/main/config/default.toml).
Theme color fields, by contrast, *do* layer on top of the chosen `preset`.
:::

## In-app settings panel

You don't have to edit TOML to change settings — open the settings panel from
the command palette (`Alt p`) and adjust things live. Changes are written back
to your `config.toml`.
