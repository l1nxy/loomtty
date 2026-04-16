use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
use super::text_layout;
use super::tokens;
use super::types::{UiComponent, UiContext, UiScene};
use crate::app::App;

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
}

pub(super) fn action_short_label(action: &str) -> &str {
    match action {
        "focus_left" => "left",
        "focus_right" => "right",
        "focus_up" => "up",
        "focus_down" => "down",
        "move_pane_left" => "move \u{2190}",
        "move_pane_right" => "move \u{2192}",
        "new_column_right" => "new pane",
        "new_row_below" | "new_workspace_below" | "split_down" => "split \u{2193}",
        "close_pane" => "close",
        "column_width_decrease" => "shrink",
        "column_width_increase" => "grow",
        "column_width_full" => "full",
        "column_width_one_third" => "1/3",
        "column_width_half" => "1/2",
        "column_width_two_thirds" => "2/3",
        "cycle_preset_width" => "next width",
        "cycle_preset_width_reverse" => "prev width",
        "equalize_adjacent_columns" => "equalize",
        "consume_into_column" => "stack",
        "expel_from_column" => "unstack",
        "toggle_broadcast" => "broadcast",
        "toggle_overview" => "overview",
        "exit_overview" => "exit",
        "toggle_command_palette" => "palette",
        "toggle_lock" => "lock",
        "detach" => "detach",
        "scroll_line_up" => "line \u{2191}",
        "scroll_line_down" => "line \u{2193}",
        "scroll_half_page_up" => "half \u{2191}",
        "scroll_half_page_down" => "half \u{2193}",
        "scroll_page_up" => "page \u{2191}",
        "scroll_page_down" => "page \u{2193}",
        "scroll_top" => "top",
        "scroll_bottom" => "bottom",
        s if s.starts_with("enter_mode:workspace") => "workspace",
        s if s.starts_with("enter_mode:session") => "session",
        s if s.starts_with("enter_mode:resize") => "resize",
        s if s.starts_with("enter_mode:move") => "move",
        s if s.starts_with("enter_mode:scroll") => "scroll",
        s if s.starts_with("enter_mode:") => s.strip_prefix("enter_mode:").unwrap_or(s),
        s if s.starts_with("switch_workspace_") => s.strip_prefix("switch_workspace_").unwrap_or(s),
        other => other,
    }
}

fn build_infobox_rows(
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
            (key_display, action_short_label(action).to_string())
        })
        .collect()
}

impl InfoBoxComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if app.core.command_palette.is_some() || app.core.pending_paste.is_some() {
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
        })
    }
}

impl UiComponent for InfoBoxComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let padding = cx.cell_w;
        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        let bw = tokens::BORDER_THIN;
        let title_h = cx.cell_h + tokens::SPACE_1;
        let content_w = self.w - bw * 2.0;

        let mut ui = UiBuilder::new_vertical(
            self.x + bw,
            self.y + bw,
            content_w,
            self.h - bw * 2.0,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        // Shadow + border + background
        let bg_color = [bg[0] * 0.85, bg[1] * 0.85, bg[2] * 0.85, 0.97];
        ui.bordered_panel_inset(self.x, self.y, self.w, self.h, bg_color, accent, bw, true);

        // Title row (tinted background + text)
        ui.horizontal(Some(content_w), title_h, 0.0, |ui| {
            let (rx, ry) = ui.cursor_pos();
            ui.abs_rect(
                rx,
                ry,
                content_w,
                title_h,
                tokens::tint(accent, tokens::ALPHA_TINT_HEADER),
            );
            let text_y = ry + (title_h - cx.cell_h) * 0.5;
            ui.abs_text(&format!(" {} ", self.title), rx + padding, text_y, accent);
        });

        ui.bg_rect(content_w, tokens::SPACE_1, [0.0; 4]); // spacing after title

        // Key-action rows — right-align keys within the shape-measured
        // key column. `self.key_col_w` is the widest shaped key; using
        // it here guarantees every key fits and the "esc" alignment
        // matches the widest entry on proportional UI fonts.
        let key_col_w = self.key_col_w;
        let gap_w = cx.cell_w * 2.0;

        for (key, desc) in &self.rows {
            ui.horizontal(Some(content_w), row_h, 0.0, |ui| {
                let (_, ry) = ui.cursor_pos();
                let text_y = ry + (row_h - cx.cell_h) * 0.5;

                // Right-align key within key column.
                let key_w = ui.text_width(key);
                let key_offset = (key_col_w - key_w).max(0.0);
                let key_x = ui.cursor_pos().0 + padding + key_offset;
                ui.abs_text(key, key_x, text_y, accent);

                // Description after key column + gap.
                let desc_x = ui.cursor_pos().0 + padding + key_col_w + gap_w;
                ui.abs_text(desc, desc_x, text_y, if key == "esc" { dim } else { fg });
            });
        }
    }
}
