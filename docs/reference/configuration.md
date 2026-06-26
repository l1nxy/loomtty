# Configuration Reference

Complete reference for `~/.config/loom/config.toml`, generated from the config
schema. The schema is the ultimate source of truth — if anything here drifts,
trust the code:

- [`crates/loom-config/src/schema.rs`](https://github.com/l1nxy/loomtty/blob/main/crates/loom-config/src/schema.rs)
- Annotated sample: [`config/default.toml`](https://github.com/l1nxy/loomtty/blob/main/config/default.toml)

All sections are optional; omitted fields fall back to the defaults below. For a
friendly introduction see [Configuration](/guide/configuration).

## Sections at a glance

| Section | Purpose |
| ------- | ------- |
| [`[font]`](#font) | Font family, size, OpenType features, metrics |
| [`[appearance]`](#appearance) | Padding, borders, focus ring, background image, opacity |
| [`[animation]`](#animation) | Animation toggle, speed preset, pane open style |
| [`[window]`](#window) | Initial window size and title |
| [`[terminal]`](#terminal) | Cursor, shell, scrollback, selection, bell |
| [`[statusbar]`](#statusbar) | Status bar position and spacing |
| [`[tabbar]`](#tabbar) | Pane-tab placement and sizing |
| [`[input]`](#input) | Input mode, leader timing, scroll/mouse |
| [`[render]`](#render) | GPU backend, present mode, alpha blending |
| [`[layout]`](#layout) | Column widths, new-pane sizing, centering |
| [`[gesture]`](#gesture) | Touchpad gestures and smooth scroll |
| [`[remote]`](#remote) | Remote attach listener and saved hosts |
| [`[session]`](#session) | Agent restore on reattach |
| [`[server]`](#server) | Daemon idle timeout |
| [`[prediction]`](#prediction) | Mosh-style predictive echo |
| [`[web]`](#web) | Browser gateway |
| [`[keys]`](#keys) | Leader and keybindings |
| [`[theme]`](#theme) | Colors and presets |

---

## `[font]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `family` | string | platform¹ | Terminal font family (monospace). |
| `size` | float | `10.0` | Font size in points. Range `1.0`–`200.0`. |
| `ui` | table | _none_ | Optional UI font override (`{ family, size }`); may be proportional. When unset, the UI reuses the terminal font. |
| `features` | string[] | `[]` | OpenType feature toggles, HarfBuzz/CSS syntax: `"liga"`, `"+ss01"`, `"-calt"`, `"zero=1"`. |
| `disable_ligatures` | enum | `never` | When to suppress ligatures: `never` / `cursor` (cursor row only) / `always`. |
| `weight` | int | _auto_ | Preferred OpenType weight `100`–`1000`. Unset picks the closest-to-400 face. |
| `adjust_cell_width` | float | `1.0` | Cell-width multiplier. Range `0.5`–`3.0`. |
| `adjust_cell_height` | float | `1.0` | Cell-height multiplier. Range `0.5`–`3.0`. |
| `adjust_underline_position` | float | `0.0` | Pixel offset for the underline (positive = lower). |
| `adjust_underline_thickness` | float | `1.0` | Underline thickness multiplier. Range `0.0`–`16.0`. |
| `adjust_strikethrough_position` | float | `0.0` | Pixel offset for the strikethrough. |
| `adjust_strikethrough_thickness` | float | `1.0` | Strikethrough thickness multiplier. Range `0.0`–`16.0`. |

¹ Default family is `Menlo` on macOS, `Consolas` on Windows, `monospace`
elsewhere.

```toml
[font]
family = "JetBrains Mono"
size = 12.0
features = ["+ss01", "-calt"]
disable_ligatures = "cursor"

[font.ui]
family = "Inter"
size = 10.0
```

## `[appearance]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `padding` | float | `4.0` | Inner padding around pane content (px). |
| `column_gap` | float | `4.0` | Gap between columns (px). |
| `border_width` | float | `2.0` | Pane border width (px). Min `0.0`. |
| `active_border_color` | string | `""` | Override focused border color (empty → theme). |
| `inactive_border_color` | string | `""` | Override unfocused border color (empty → theme). |
| `inactive_opacity` | float | `0.7` | Opacity of unfocused panes. Range `0.0`–`1.0`. |
| `pane_corner_radius` | float | `0.0` | Corner radius for panes (px). `0` = sharp corners. Range `0.0`–`64.0`. |
| `background_image` | string | `""` | Path to a background image (PNG/JPEG). Supports `~`, absolute, and config-relative paths. Empty = solid fill. |
| `background_dim` | float | `0.0` | Dim over the background image. `0` = full strength, `1` = hidden. Range `0.0`–`1.0`. |
| `pane_opacity` | float | `1.0` | Pane background opacity; lower values let the background image show through. Range `0.0`–`1.0`. |

### `[appearance.focus_ring]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `style` | enum | `solid` | `solid` / `glow` / `dashed`. |
| `glow_radius` | float | `4.0` | Glow radius (px), `glow` style. |
| `glow_layers` | int | `3` | Number of glow layers, `glow` style. |
| `dash_length` | float | `8.0` | Dash length (px), `dashed` style. |
| `gap_length` | float | `4.0` | Gap length (px), `dashed` style. |

## `[animation]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `enabled` | bool | `true` | Master animation toggle. |
| `preset` | enum | `default` | Speed preset: `snappy` (~0.3s) / `default` (~0.5s) / `smooth` (~0.7s) / `gentle` (~1.0s). |
| `pane_open_style` | enum | `fade` | `fade` / `slide-up` / `slide-down` / `slide-left` / `fade-slide-up`. |
| `overview_zoom_fit` | float | `0.9` | Zoom-to-fit factor in overview. Range `0`–`1`. |
| `zoom_threshold` | float | `0.99` | Zoom threshold below which overview thumbnails render. Range `0`–`1`. |
| `drag_opacity` | float | `0.6` | Opacity of a pane while being dragged. Range `0`–`1`. |

## `[window]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `width` | float | `1024.0` | Initial window width (px). |
| `height` | float | `768.0` | Initial window height (px). |
| `title` | string | `"loomtty"` | Window title. |

## `[terminal]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `default_cols` | int | `80` | Initial columns. Min `1`. |
| `default_rows` | int | `24` | Initial rows. Min `1`. |
| `cursor_color` | string | `"#E6E6E6"` | Cursor color (hex). |
| `cursor_opacity` | float | `0.7` | Cursor opacity. Range `0.0`–`1.0`. |
| `cursor_blink` | bool | `true` | Whether the cursor blinks. |
| `cursor_blink_interval_ms` | int | `500` | Blink interval (ms). Min `1`. |
| `cursor_shape` | string | `"beam"` | `block` / `beam` / `underline` / `hollow_block`; empty = follow the app. |
| `shell` | string | `""` | Shell to spawn. Empty = platform default (`$SHELL` / `cmd.exe`). |
| `scrollback_lines` | int | `10000` | Scrollback buffer size. Min `1`. |
| `copy_on_select` | bool | `false` | Copy to clipboard on selection. |
| `clear_selection_on_type` | bool | `true` | Clear the selection when you type. |
| `notify_command_threshold_secs` | int | `0` | Notify when a command runs longer than this (secs). `0` = off. Needs [shell integration](/guide/shell-integration). |
| `bell_audio` | string | `""` | Path to an audio file played on the terminal bell. |
| `bell_urgency` | bool | `true` | Set the window urgency hint on bell. |
| `paste_warn_threshold` | int | `5000` | Warn before pasting more than this many bytes. |

## `[statusbar]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `position` | enum | `top` | `top` / `bottom`. |
| `padding_ratio` | float | `0.25` | Vertical padding as a ratio of text height. Min `0.0`. |
| `text_baseline` | float | `0.8` | Text baseline position within the bar. |
| `leader_indicator_ratio` | float | `0.1` | Size of the leader indicator. |
| `height_padding` | float | _auto_ | Optional extra bar height (px). |

## `[tabbar]`

Controls the per-pane tab strip.

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `position` | enum | `integrated` | `integrated` (inside the status bar) / `left` / `right` (a dedicated side bar). |
| `width` | float | `200.0` | Side-bar width (px), `left`/`right` only. Range `40`–`800`. |
| `tab_height` | float | `36.0` | Per-tab height (px), `left`/`right` only. Range `12`–`200`. |
| `tab_gap` | float | `4.0` | Gap between tabs (px), `left`/`right` only. Range `0`–`40`. |
| `pane_tab_width_chars` | int | `25` | Per-tab width in characters, `integrated` only. Range `4`–`80`. |

## `[input]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `mode` | enum | `sticky` | `sticky` (zellij-style hold) / `prefix` (tmux-style tap). See [Keybindings](/guide/keybindings). |
| `leader_timeout_ms` | int | `1000` | How long the leader stays armed in `prefix` mode (ms). |
| `double_tap_window_ms` | int | `300` | Double-tap detection window (ms). |
| `scroll_multiplier` | float | `50.0` | Mouse-wheel scroll multiplier. |
| `focus_follows_mouse` | bool | `false` | Focus the pane under the pointer. |

## `[render]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `backend` | enum | `auto` | GPU backend: `auto` (platform default) / `blade` (Vulkan/Metal/DirectX 11) / `gl` (OpenGL). |
| `present_mode` | enum | `fifo` | `fifo` (vsync) / `mailbox` / `immediate`. |
| `alpha_blending` | enum | platform² | `native` (sRGB) / `linear` / `linear-corrected`. |
| `softness` | float | `0.0` | Post-process softness `0`–`1`. `0` = off. GL backend + linear blending only. |
| `frame_interval_ms` | int | `16` | Target frame interval (ms). Min `1`. |
| `frame_latency` | int | `2` | Frames of GPU latency to allow. |
| `atlas_size` | int | `2048` | Glyph atlas texture size (px). |
| `max_glyph_instances` | int | `32768` | Glyph instance buffer capacity. |
| `max_rectangles` | int | `8192` | Rectangle instance buffer capacity. |

² Default `alpha_blending` is `linear-corrected` on Linux/Windows, `native` on
macOS.

## `[layout]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `new_pane_sizing` | enum | `fixed` | `fixed` (constant width) / `dynamic` (full below a width threshold, else half). |
| `new_pane_width` | enum | `half` | In `fixed` mode: `half` or `full` viewport width. |
| `dynamic_fullscreen_max_width` | float | `1000.0` | In `dynamic` mode: window width (px) below which new panes open full-width. |
| `default_column_width` | preset³ | _none_ | Power-user override: force every new column to a fixed width. Wins over the `new_pane_*` policy. |
| `preset_widths` | preset³[] | ⅓, ½, ⅔, full | Widths cycled by `cycle_preset_width` in resize mode. |
| `center_focused_column` | enum | `never` | `never` (minimal scroll) / `on-overflow` (PaperWM-style) / `always`. |

³ A **preset width** is `{ proportion = 0.5 }` (fraction of the viewport) or
`{ fixed = 600.0 }` (pixels).

```toml
[layout]
new_pane_sizing = "dynamic"
dynamic_fullscreen_max_width = 1100.0
center_focused_column = "on-overflow"
preset_widths = [
  { proportion = 0.333 },
  { proportion = 0.5 },
  { proportion = 0.667 },
  { proportion = 1.0 },
]
```

## `[gesture]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `enabled` | bool | `true` | Enable touchpad gestures. |
| `pinch_sensitivity` | float | `2.0` | Pinch-to-zoom sensitivity. |
| `natural_scroll` | bool | `true` | Natural (reversed) scroll direction. |
| `vertical_swipe_threshold` | float | `50.0` | Vertical swipe activation distance. |
| `horizontal_swipe_threshold` | float | `50.0` | Horizontal swipe activation distance. |
| `smooth_scroll` | bool | `true` | Smooth (pixel) scrolling. |
| `scroll_pixels_per_line` | float | `20.0` | Pixels per line for discrete scroll. |

## `[remote]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `enabled` | bool | `false` | Listen for remote attach connections. |
| `port` | int | `7890` | Listener port. |
| `hosts` | table[] | `[]` | Saved hosts shown in the session palette. |

Each entry in `hosts` is:

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `name` | string | — | Display name in the palette. |
| `host` | string | — | Hostname or IP. |
| `port` | int | `7890` | Remote loomtty port. |
| `ssh_port` | int | `22` | SSH port for the tunnel. |

```toml
[remote]
enabled = true
port = 7890

[[remote.hosts]]
name = "devbox"
host = "dev.example.com"
ssh_port = 22
```

See [Remote & Predictive Echo](/guide/remote-attach).

## `[session]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `restore_agents` | bool | `true` | Detect and auto-resume AI agent panes (Claude Code, Codex, opencode, Droid) on restart. |
| `agent_save_interval_secs` | int | `30` | Agent-state autosave interval (secs). |

## `[server]`

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `idle_timeout_secs` | int | `300` | Seconds the daemon lingers after the last session/client exits. `0` = shut down immediately. |

## `[prediction]`

Mosh-style speculative local echo for high-latency links.

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `mode` | enum | `never` | `never` / `always` / `adaptive` (predict only when latency warrants). |
| `threshold_ms` | int | `30` | Latency above which `adaptive` starts predicting (ms). |
| `show_underline` | bool | `true` | Underline predicted (not-yet-confirmed) characters. |

See [Remote & Predictive Echo](/guide/remote-attach).

## `[web]`

Browser gateway — disabled by default. The easiest way to enable it is the
`loomtty web` command, which mints a token and prints the URL. See [Web UI](/guide/web-ui).

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `enabled` | bool | `false` | Serve the browser SPA + `/ws` gateway. |
| `bind` | string | `""` | Bind address (IP). Empty = `127.0.0.1`. |
| `port` | int | `7891` | Listen port. `0` is rejected. |
| `token` | string | `""` | Shared auth token. **Required** and ≥ 16 bytes when `enabled = true` (unless `auth = "none"`). Generate with `openssl rand -hex 16`. |
| `auth` | string | `"token"` | `"token"` or `"none"`. `"none"` is **demo mode**: no token, the browser UI skips the login screen. Only honored on a loopback `bind`. |
| `allowed_origins` | string[] | `[]` | Exact-match `Origin` allowlist (CSRF defense). See below. |
| `static_dir` | string | `""` | Directory the SPA is served from. Empty = `<server-exe-dir>/web`. |

::: danger Security
- The gateway speaks plain `http` / `ws`. Put TLS in front before exposing it
  past loopback.
- A **non-loopback `bind` with an empty `allowed_origins`** makes the daemon
  refuse to start — set the browser origin(s) explicitly, e.g.
  `["https://terminal.example.com"]` (lowercase; `http`/`https` only, or the
  literal `"null"` for sandboxed contexts).
- `static_dir` is served **without** the token, so put only the public web
  bundle there — never secrets.
:::

## `[keys]`

The leader key and all keybinding tables. See [Keybindings](/guide/keybindings)
for the full default map and [Actions](/reference/actions) for every bindable
action.

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `leader` | string | `"alt"` | Leader key. |
| `bindings` | map | _see defaults_ | Normal (leader) key → action. |
| `modes` | map of maps | _see defaults_ | Named key tables (`resize`, `scroll`, `move`). |
| `overview_bindings` | map | _see defaults_ | Keys active in overview. |
| `direct_bindings` | map | _see defaults_ | Keys that work without the leader. |
| `search_bindings` | map | _see defaults_ | Keys active in the search bar. |
| `palette_bindings` | map | _see defaults_ | Keys active in the command palette. |
| `paste_confirm_bindings` | map | _see defaults_ | Keys active in the paste dialog. |

::: warning
Defining one of these maps **replaces the whole default map** — it isn't merged.
See [Keybindings → Customizing](/guide/keybindings#customizing).
:::

## `[theme]`

Color preset plus per-color overrides. The full field list and preset names are
in [Theming](/guide/theming).

| Key | Type | Default | Description |
| --- | ---- | ------- | ----------- |
| `preset` | string | `"loom_dark"` | One of the nine built-in presets. |
| _color fields_ | string | _from preset_ | Any terminal/UI/chrome color as `#RRGGBB`; overrides the preset. |
