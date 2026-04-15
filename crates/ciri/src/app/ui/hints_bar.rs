use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
use super::info_box::action_short_label;
use super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;

struct HintItem {
    key: String,
    label: String,
}

pub(crate) struct HintsBarComponent {
    bar_h: f32,
    pane_count: usize,
    active_pane_title: String,
    hints: Vec<HintItem>,
}

impl HintsBarComponent {
    pub fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
        let bar_h = app.hints_bar_height();

        let ws = app.core.workspaces.active();
        let pane_count = ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>();
        let active_pane_title = ws
            .active_pane_id()
            .and_then(|id| app.core.pane_grids.get(&id))
            .map(|g| g.title.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();

        let hints = Self::pick_hints(app);

        Self {
            bar_h,
            pane_count,
            active_pane_title,
            hints,
        }
    }

    fn pick_hints(app: &App) -> Vec<HintItem> {
        let h = |key: &str, label: &str| HintItem {
            key: key.into(),
            label: label.into(),
        };

        let find_key = |action: &str,
                        bindings: &std::collections::HashMap<String, String>|
         -> Option<String> {
            bindings
                .iter()
                .find(|(_, v)| v.as_str() == action)
                .map(|(k, _)| k.clone())
        };

        if app.core.input.is_locked() {
            let key = find_key("toggle_lock", &app.core.config.keys.direct_bindings)
                .or_else(|| find_key("toggle_lock", &app.core.config.keys.bindings))
                .unwrap_or_else(|| "g".into());
            return vec![h(&key, "unlock")];
        }

        if app.core.overview.active {
            return vec![h("hjkl", "move"), h("enter", "select"), h("esc", "exit")];
        }

        if let Some(mode_name) = app.core.input.current_mode_name() {
            let mut hints = Vec::new();
            if let Some(mode_bindings) = app.core.config.keys.modes.get(mode_name) {
                let mut entries: Vec<_> = mode_bindings.iter().collect();
                entries.sort_by_key(|(k, _)| k.len());
                for (key, action) in entries.into_iter().take(3) {
                    hints.push(h(key, action_short_label(action)));
                }
            }
            hints.push(h("esc", "exit"));
            return hints;
        }

        if app.core.input.is_awaiting_action() {
            let bindings = &app.core.config.keys.bindings;
            let mut hints = Vec::new();
            for (action, label) in [
                ("new_column_right", "new"),
                ("close_pane", "close"),
                ("toggle_overview", "overview"),
                ("toggle_command_palette", "palette"),
            ] {
                if let Some(key) = find_key(action, bindings) {
                    hints.push(h(&key, label));
                }
            }
            return hints;
        }

        let mut hints = Vec::new();
        if !app.core.input.uses_bare_modifier_promotion() {
            hints.push(h(&app.core.config.keys.leader, "leader"));
        }
        let direct = &app.core.input.direct_keybinds;
        for (action, label) in [("toggle_help", "help"), ("toggle_lock", "lock")] {
            if let Some(key_display) = direct.find_key_for_action(action) {
                hints.push(h(&key_display, label));
            }
        }
        hints
    }
}

impl UiElement for HintsBarComponent {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Vertical => SizeHint::Fixed(self.bar_h),
            Axis::Horizontal => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let padding = cx.cell_w;

        // Text row is vertically centered within the bar
        let text_y = rect.y + (rect.h - cx.cell_h) * 0.5;

        // Builder is pinned to the inner (padded) content strip. Absolute
        // rects use full rect coords so the background + separator span
        // edge to edge.
        let mut ui = UiBuilder::new_horizontal(
            rect.x + padding,
            text_y,
            (rect.w - padding * 2.0).max(0.0),
            cx.cell_h,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );
        ui.abs_rect(rect.x, rect.y, rect.w, rect.h, bar_bg);
        ui.abs_rect(rect.x, rect.y, rect.w, tokens::BORDER_THIN, sep_color);

        // Pre-compute hints total width for right-alignment
        let hint_spacing = cx.cell_w * 2.0;
        let max_hints_w = rect.w * 0.6;
        let mut hints_w = 0.0_f32;
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                hints_w += hint_spacing;
            }
            hints_w += ui.text_width(&item.key) + ui.text_width(&format!(" {}", item.label));
        }
        hints_w = hints_w.min(max_hints_w);

        // Left side fills remaining space after reserving hints width
        let left_w = ui.remaining() - hints_w;
        ui.horizontal(Some(left_w.max(0.0)), cx.cell_h, 0.0, |ui| {
            ui.label("\u{25CF} ", accent);

            let pane_text = if self.pane_count == 1 {
                "1 pane".to_string()
            } else {
                format!("{} panes", self.pane_count)
            };
            ui.label(&pane_text, dim);

            if !self.active_pane_title.is_empty() {
                ui.label(" \u{00B7} ", dim);
                ui.label(&self.active_pane_title, fg);
            }
        });

        // Right side: hints (key in accent, label in dim)
        ui.horizontal(Some(hints_w), cx.cell_h, 0.0, |ui| {
            for (i, item) in self.hints.iter().enumerate() {
                if i > 0 {
                    ui.label("  ", dim);
                }
                ui.label(&item.key, accent);
                ui.label(&format!(" {}", item.label), dim);
            }
        });
    }
}
