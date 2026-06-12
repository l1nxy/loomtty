# Theming

loomtty ships nine built-in color presets. Pick one with `theme.preset`, then
override any individual color you like — overrides are layered **on top of** the
preset, so you only specify what you want to change.

## Presets

```toml
[theme]
preset = "loom_dark"
```

| Preset | |
| ------ | --- |
| `loom_dark` | loomtty's own dark theme (default) |
| `one_dark` | Atom One Dark |
| `one_half_dark` | OneHalf Dark |
| `catppuccin_mocha` | Catppuccin Mocha |
| `tokyo_night` | Tokyo Night |
| `dracula` | Dracula |
| `nord` | Nord |
| `gruvbox_dark` | Gruvbox Dark |
| `ghostty` | Ghostty's default palette |

An unknown preset name falls back to `loom_dark` (with a warning). You can also
switch themes live from the in-app settings panel.

## Colors are hex strings

Every color is a 6-digit hex string with a leading `#`, e.g. `"#1C1B1A"`.
3-digit shorthand and 8-digit (alpha) hex are **not** supported; an invalid value
falls back to light gray with a warning.

## Overriding colors

Set `preset`, then override any field below it:

```toml
[theme]
preset = "catppuccin_mocha"
# Tweaks on top of the preset:
background = "#11111b"
accent = "#89b4fa"
cursor_color = "#f5e0dc"   # note: cursor_color lives under [terminal]
```

## Terminal palette

The standard 16 ANSI colors plus foreground/background:

| Field | Field | Field |
| ----- | ----- | ----- |
| `foreground` | `background` | |
| `black` | `red` | `green` |
| `yellow` | `blue` | `magenta` |
| `cyan` | `white` | |
| `bright_black` | `bright_red` | `bright_green` |
| `bright_yellow` | `bright_blue` | `bright_magenta` |
| `bright_cyan` | `bright_white` | |

## UI palette

Colors for loomtty's own chrome (status bar, borders, overview, mode badges):

| Field | What it colors |
| ----- | -------------- |
| `overview_background` | Backdrop behind panes and in overview |
| `statusbar_background` | Status bar background |
| `statusbar_dim` | Dim/secondary status bar text |
| `border_active` | Focused pane border |
| `border_inactive` | Unfocused pane borders |
| `accent` | Leader indicator, active mode text |
| `mode_broadcast` | Broadcast-mode indicator |

## Chrome palette

Floating surfaces — the command palette, dialogs, context menus, and status
hues — have their own palette so they stay visually consistent when you swap
terminal themes. Each field is **optional**; when unset it uses a
preset-independent loom default.

| Field | Default | Role |
| ----- | ------- | ---- |
| `ui_surface` | `#1A1816` | Floating panel / dialog background |
| `ui_on_surface` | `#E2DCD6` | Default chrome text |
| `ui_on_surface_muted` | `#8E8780` | Muted chrome text (descriptions, hints) |
| `ui_border` | `#2A2622` | Panel edge / outer border |
| `ui_element_hover` | `#22201E` | Hovered row background |
| `ui_element_active` | `#2A2724` | Pressed / active row background |
| `ui_error` | `#E27870` | Error status (warm coral) |
| `ui_warning` | `#E6B26B` | Warning status (warm amber) |
| `ui_success` | `#A0BC75` | Success status (sage green) |
| `ui_info` | `#7DAEC8` | Info status (soft sky blue) |

## Related appearance settings

A few visual knobs live outside `[theme]`:

- **Borders & focus ring** — `[appearance]`: `border_width`, `focus_ring.style`
  (`solid` / `glow` / `dashed`), `pane_corner_radius`. See
  [appearance reference](/reference/configuration#appearance).
- **Cursor** — `[terminal]`: `cursor_color`, `cursor_shape`, `cursor_blink`,
  `cursor_opacity`. See [terminal reference](/reference/configuration#terminal).
- **Background image & translucency** — `[appearance]`: `background_image`,
  `background_dim`, `pane_opacity`. Drop in a wallpaper and let panes show
  through.

```toml
[appearance]
background_image = "~/Pictures/wallpaper.jpg"
background_dim = 0.5    # 0 = full strength, 1 = hidden
pane_opacity = 0.85     # let the wallpaper show through panes
pane_corner_radius = 8  # rounded panes
```

See the [Configuration reference](/reference/configuration) for every field.
