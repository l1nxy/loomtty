use loom_config::config::StatusBarPosition;

use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{Div, ElevationIndex, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

pub(crate) struct InfoBoxComponent {
    title: String,
    rows: Vec<(String, String)>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Shape-measured pixel width of the widest key; reused by `paint()`
    /// so key-column right-alignment matches the shaped glyph advance
    /// rather than a cell-grid estimate.
    key_col_w: f32,
    /// Cell metrics frozen at `capture()` time so the `Render` impl
    /// can build its tree from a minimal `RenderCtx` without re-borrowing
    /// the host shaper. Same pattern as `PaletteComponent::ui_line_h`.
    cell_w: f32,
    cell_h: f32,
}

/// Short, human-readable label for an action name (the config-format
/// `snake_case` string). Used by the hints bar and the keybindings help
/// overlay. Hand-tuned for the common actions; anything unmapped —
/// including `enter_mode:<name>` / `switch_workspace_<n>` and any custom
/// or future action — is humanized (`snake_case` → "snake case") so a
/// binding NEVER renders a raw underscored action id.
pub(super) fn action_short_label(action: &str) -> String {
    let mapped: Option<&str> = match action {
        "focus_left" => Some("left"),
        "focus_right" => Some("right"),
        "focus_up" => Some("up"),
        "focus_down" => Some("down"),
        "move_pane_left" => Some("move \u{2190}"),
        "move_pane_right" => Some("move \u{2192}"),
        "move_pane_up" => Some("move \u{2191}"),
        "move_pane_down" => Some("move \u{2193}"),
        "new_column_right" => Some("new pane"),
        "new_row_below" | "new_workspace_below" | "split_down" => Some("split \u{2193}"),
        "new_tile_below" | "stack_pane" => Some("stack \u{2193}"),
        "close_pane" => Some("close"),
        "column_width_decrease" => Some("shrink"),
        "column_width_increase" => Some("grow"),
        "column_width_full" => Some("full"),
        "column_width_one_third" => Some("1/3"),
        "column_width_half" => Some("1/2"),
        "column_width_two_thirds" => Some("2/3"),
        "cycle_preset_width" => Some("next width"),
        "cycle_preset_width_reverse" => Some("prev width"),
        "equalize_adjacent_columns" => Some("equalize"),
        "consume_into_column" => Some("stack"),
        "expel_from_column" => Some("unstack"),
        "tile_height_increase" => Some("taller"),
        "tile_height_decrease" => Some("shorter"),
        "toggle_broadcast" => Some("broadcast"),
        "toggle_overview" => Some("overview"),
        "exit_overview" => Some("exit"),
        "toggle_command_palette" => Some("palette"),
        "toggle_session_palette" => Some("sessions"),
        "toggle_lock" => Some("lock"),
        "toggle_help" => Some("help"),
        "toggle_settings" | "settings" => Some("settings"),
        "next_session" => Some("next session"),
        "prev_session" | "previous_session" => Some("prev session"),
        "new_session" => Some("new session"),
        "detach" => Some("detach"),
        "open_search" => Some("search"),
        "close_search" => Some("close search"),
        "search_next_match" => Some("next match"),
        "search_prev_match" => Some("prev match"),
        "clipboard_copy" => Some("copy"),
        "clipboard_paste" => Some("paste"),
        "send_leader_key" => Some("send leader"),
        "scroll_line_up" => Some("line \u{2191}"),
        "scroll_line_down" => Some("line \u{2193}"),
        "scroll_half_page_up" => Some("half \u{2191}"),
        "scroll_half_page_down" => Some("half \u{2193}"),
        "scroll_page_up" => Some("page \u{2191}"),
        "scroll_page_down" => Some("page \u{2193}"),
        "scroll_top" => Some("top"),
        "scroll_bottom" => Some("bottom"),
        "prev_prompt" | "previous_prompt" => Some("prev prompt"),
        "next_prompt" => Some("next prompt"),
        _ => None,
    };
    if let Some(label) = mapped {
        return label.to_string();
    }
    // Prefixed dynamic actions: label by their argument.
    if let Some(rest) = action.strip_prefix("enter_mode:") {
        return humanize(rest);
    }
    if let Some(rest) = action.strip_prefix("switch_workspace_") {
        return humanize(rest);
    }
    // Last resort: humanize so no raw `snake_case` ever reaches the UI.
    humanize(action)
}

/// Turn a `snake_case` action id into a space-separated label.
fn humanize(s: &str) -> String {
    s.replace('_', " ")
}

pub(super) fn build_infobox_rows(
    bindings: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    use std::collections::HashMap;
    let mut action_to_keys: HashMap<&str, Vec<&str>> = HashMap::new();
    for (key, action) in bindings {
        action_to_keys
            .entry(action.as_str())
            .or_default()
            .push(key.as_str());
    }
    for keys in action_to_keys.values_mut() {
        keys.sort_by_key(|k| k.len());
    }
    let mut entries: Vec<_> = action_to_keys.into_iter().collect();
    entries.sort_by(|(_, a_keys), (_, b_keys)| {
        a_keys[0]
            .len()
            .cmp(&b_keys[0].len())
            .then(a_keys[0].cmp(b_keys[0]))
    });

    entries
        .into_iter()
        .map(|(action, keys)| {
            let key_display = if keys.len() <= 2 {
                keys.join("/")
            } else {
                keys[..2].join("/")
            };
            (key_display, action_short_label(action))
        })
        .collect()
}

impl InfoBoxComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        // Hide while another modal-ish overlay owns user attention.
        // Includes context_menu — the GPU pipeline batches by primitive
        // type (all rects, then all glyphs), so a later context_menu's
        // bg rect can't actually cover an earlier InfoBox's glyphs in
        // the merged scene. Suppressing InfoBox at capture time avoids
        // the bleed-through entirely (rather than fighting it at paint).
        if app.core.command_palette.is_some()
            || app.core.pending_paste.is_some()
            || app.core.context_menu.visible
            || app.core.settings_panel_visible
            || app.core.help_visible
        {
            return None;
        }

        let (title, bindings) = if let Some(mode_name) = app.core.input.current_mode_name() {
            let bindings = app.core.config.keys.modes.get(mode_name)?;
            (mode_name.to_uppercase(), bindings.clone())
        } else if app.core.input.is_awaiting_action() {
            ("LEADER".to_string(), app.core.config.keys.bindings.clone())
        } else {
            return None;
        };

        let mut rows = build_infobox_rows(&bindings);
        rows.push(("esc".to_string(), "exit".to_string()));

        let padding = cx.cell_w;
        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        // All column widths are measured from the shaped glyph advance;
        // this is what the renderer will actually draw, so the box is
        // always wide enough and the key column's right-edge alignment
        // holds on proportional UI fonts too.
        let key_col_w = rows
            .iter()
            .map(|(k, _)| text_layout::measure(cx, k))
            .fold(0.0_f32, f32::max);
        let val_col_w = rows
            .iter()
            .map(|(_, v)| text_layout::measure(cx, v))
            .fold(0.0_f32, f32::max);
        // Title row: ` {title} ` with an extra cell of padding either side.
        let title_box_w = text_layout::measure(cx, &title) + cx.cell_w * 4.0;
        // Content row: key-col + 2 cells of gap (matches `gap_w` in
        // `paint`) + val-col.
        let content_box_w = key_col_w + cx.cell_w * 2.0 + val_col_w;
        let inner_w = content_box_w.max(title_box_w);
        let w = inner_w + padding * 2.0;
        let h = rows.len() as f32 * row_h + padding * 2.0 + cx.cell_h;

        let hints_bar_h = app.hints_bar_height();
        let status_bar_h = app.status_bar_height();
        let margin = tokens::SPACE_2;
        let x = cx.viewport_w - w - margin;
        let bottom_chrome = match cx.config.statusbar.position {
            StatusBarPosition::Top => hints_bar_h,
            StatusBarPosition::Bottom => status_bar_h + hints_bar_h,
        };
        let y = cx.viewport_h - h - bottom_chrome - margin;

        Some(Self {
            title,
            rows,
            x,
            y,
            w,
            h,
            key_col_w,
            cell_w: cx.cell_w,
            cell_h: cx.cell_h,
        })
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }
}

impl InfoBoxComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // Fourth production usage of `loom_ui::Render` after palette,
        // context_menu, and paste_dialog.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let accent = cx.theme.accent;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let padding = self.cell_w;
        let row_h = self.cell_h + tokens::SPACE_1 * 2.0;
        let bw = tokens::BORDER_THIN;
        let title_h = self.cell_h + tokens::SPACE_1;
        let content_w = self.w - bw * 2.0;

        // Info box sits at the `Panel` elevation tier (sunk surface) —
        // same as palette / context_menu / connection_status banner.
        // The `0.97` alpha keeps a hint of pane bleed-through for the
        // overlay-on-pane feel.
        let panel = ElevationIndex::Panel.bg(cx.theme);
        let bg_color = [panel[0], panel[1], panel[2], 0.97];

        // Key-action rows — right-align keys within the shape-measured
        // key column. `self.key_col_w` is the widest shaped key; using
        // it here guarantees every key fits and the "esc" alignment
        // matches the widest entry on proportional UI fonts.
        let key_col_w = self.key_col_w;
        let gap_w = self.cell_w * 2.0;
        let mut panel = div()
            .absolute()
            .left(self.x)
            .top(self.y)
            .w(self.w)
            .h(self.h)
            .flex_col()
            .items_center()
            .bg(bg_color)
            .rounded(cx.theme.radius.md)
            // Border goes neutral (chrome `border`) to match palette /
            // context_menu / dialog. Mode identity comes from the title
            // row text (already painted inside this panel) — no need for
            // an accent edge that drifts hue per preset.
            .border(bw, cx.theme.border)
            .shadow_md()
            .child(
                div()
                    .w(content_w)
                    .h(title_h)
                    .flex_row()
                    .items_center()
                    .bg(tokens::tint(accent, tokens::ALPHA_TINT_HEADER))
                    .child(div().w(padding).h(title_h))
                    .child(text(format!(" {} ", self.title)).color(accent)),
            )
            .child(div().w(content_w).h(tokens::SPACE_1));
        for (key, desc) in &self.rows {
            panel = panel.child(
                div()
                    .w(content_w)
                    .h(row_h)
                    .flex_row()
                    .items_center()
                    .child(div().w(padding).h(row_h))
                    .child(
                        div()
                            .w(key_col_w)
                            .h(row_h)
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .child(text(key.clone()).color(accent)),
                    )
                    .child(div().w(gap_w).h(row_h))
                    .child(text(desc.clone()).color(if key == "esc" { dim } else { fg })),
            );
        }

        // `deferred()` keeps the panel z-on-top of any other Chrome
        // primitives the host appends after this widget — no scrim
        // here (info-box is non-modal), just paint-order escape.
        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(deferred(panel))
    }
}

impl Render for InfoBoxComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::action_short_label;
    use loom_config::config::LoomConfig;

    /// Every action reachable from the shipped default keymap must render
    /// a clean label — no raw `snake_case` ids leaking into the hints bar
    /// or the help overlay. Guards the `action_short_label` table + its
    /// humanizing fallback against drift when new default bindings land.
    #[test]
    fn default_keymap_labels_have_no_underscores() {
        let keys = LoomConfig::default().keys;
        let mut actions: Vec<&str> = Vec::new();
        actions.extend(keys.bindings.values().map(String::as_str));
        actions.extend(keys.overview_bindings.values().map(String::as_str));
        actions.extend(keys.direct_bindings.values().map(String::as_str));
        for table in keys.modes.values() {
            actions.extend(table.values().map(String::as_str));
        }

        for action in actions {
            let label = action_short_label(action);
            assert!(
                !label.is_empty(),
                "action {action:?} produced an empty label",
            );
            assert!(
                !label.contains('_'),
                "action {action:?} -> label {label:?} still shows underscores",
            );
        }
    }

    /// The fallback humanizes any unmapped / custom / future action id so
    /// it never reaches the UI as raw `snake_case`.
    #[test]
    fn unknown_action_is_humanized() {
        assert_eq!(
            action_short_label("some_future_action"),
            "some future action"
        );
        assert_eq!(action_short_label("enter_mode:my_custom"), "my custom");
        assert_eq!(action_short_label("switch_workspace_3"), "3");
    }
}
