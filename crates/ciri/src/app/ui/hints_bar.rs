use ciri_config::theme::ThemeConfig;
use ciri_render::rect::Rect;
use unicode_width::UnicodeWidthStr;

use super::info_box::action_short_label;
use super::types::{UiComponent, UiContext, UiScene};
use crate::app::status_bar::{TextEmitParams, emit_status_text};
use crate::app::App;

struct HintItem {
    key: String,
    label: String,
}

pub(crate) struct HintsBarComponent {
    bar_y: f32,
    bar_h: f32,
    pane_count: usize,
    active_pane_title: String,
    hints: Vec<HintItem>,
}

impl HintsBarComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Self {
        let bar_h = app.hints_bar_height();
        let bar_y = app.hints_bar_y(cx.viewport_h);

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
            bar_y,
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

impl UiComponent for HintsBarComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);

        scene.bg_rects.push(Rect {
            x: 0.0,
            y: self.bar_y,
            w: cx.viewport_w,
            h: self.bar_h,
            color: bar_bg,
        });

        scene.bg_rects.push(Rect {
            x: 0.0,
            y: self.bar_y,
            w: cx.viewport_w,
            h: 1.0,
            color: [dim[0], dim[1], dim[2], 0.25],
        });

        let text_y = self.bar_y + (self.bar_h - cx.cell_h) * 0.5;
        let padding = cx.cell_w;

        let mut x = padding;

        emit_status_text(
            scene.atlas,
            "\u{25CF}",
            &TextEmitParams {
                x_start: x,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: accent,
            },
            scene.glyphs,
        );
        x += cx.cell_w * 2.0;

        let pane_text = if self.pane_count == 1 {
            "1 pane".to_string()
        } else {
            format!("{} panes", self.pane_count)
        };
        emit_status_text(
            scene.atlas,
            &pane_text,
            &TextEmitParams {
                x_start: x,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: dim,
            },
            scene.glyphs,
        );
        x += UnicodeWidthStr::width(pane_text.as_str()) as f32 * cx.cell_w;

        if !self.active_pane_title.is_empty() {
            let sep = " \u{00B7} ";
            emit_status_text(
                scene.atlas,
                sep,
                &TextEmitParams {
                    x_start: x,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: dim,
                },
                scene.glyphs,
            );
            x += UnicodeWidthStr::width(sep) as f32 * cx.cell_w;

            let title = self.active_pane_title.clone();
            emit_status_text(
                scene.atlas,
                &title,
                &TextEmitParams {
                    x_start: x,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: fg,
                },
                scene.glyphs,
            );
        }

        let hint_spacing = cx.cell_w * 2.0;
        let mut total_hints_w = 0.0_f32;
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                total_hints_w += hint_spacing;
            }
            total_hints_w +=
                (item.key.chars().count() + 1 + item.label.chars().count()) as f32 * cx.cell_w;
        }

        let max_hints_w = cx.viewport_w * 0.6;
        let mut rx = cx.viewport_w - padding - total_hints_w.min(max_hints_w);

        for (i, item) in self.hints.iter().enumerate() {
            if rx > cx.viewport_w - padding {
                break;
            }
            if i > 0 {
                rx += hint_spacing;
            }

            emit_status_text(
                scene.atlas,
                &item.key,
                &TextEmitParams {
                    x_start: rx,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: accent,
                },
                scene.glyphs,
            );
            rx += item.key.chars().count() as f32 * cx.cell_w;

            let label_text = format!(" {}", item.label);
            emit_status_text(
                scene.atlas,
                &label_text,
                &TextEmitParams {
                    x_start: rx,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: dim,
                },
                scene.glyphs,
            );
            rx += label_text.chars().count() as f32 * cx.cell_w;
        }
    }
}
