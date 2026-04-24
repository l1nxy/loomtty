use super::info_box::action_short_label;
use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiRect, UiScene};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

struct HintItem {
    key: String,
    label: String,
}

pub(crate) struct HintsBarComponent {
    pane_count: usize,
    active_pane_title: String,
    hints: Vec<HintItem>,
}

impl HintsBarComponent {
    pub fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
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

impl HintsBarComponent {
    pub(crate) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_element_tree(&root, cx, scene);
    }
}

impl HintsBarComponent {
    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let bar_bg = cx.theme.statusbar_bg;
        let accent = cx.theme.accent;
        let dim = cx.theme.on_surface_muted;
        let fg = cx.theme.on_surface;
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let padding = cx.cell_w;

        // Pre-compute hints total width for right-alignment
        let hint_spacing = cx.cell_w * 2.0;
        let max_hints_w = rect.w * 0.6;
        let mut hints_w = 0.0_f32;
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                hints_w += hint_spacing;
            }
            hints_w += text_layout::measure(cx, &item.key)
                + text_layout::measure(cx, &format!(" {}", item.label));
        }
        hints_w = hints_w.min(max_hints_w);

        // Left side fills remaining space after reserving hints width
        let inner_w = (rect.w - padding * 2.0).max(0.0);
        let left_w = (inner_w - hints_w).max(0.0);
        let pane_text = if self.pane_count == 1 {
            "1 pane".to_string()
        } else {
            format!("{} panes", self.pane_count)
        };
        let mut left = div()
            .w(left_w)
            .h(cx.cell_h)
            .flex_row()
            .items_center()
            .child(text("\u{25CF} ").color(accent))
            .child(text(pane_text).color(dim));
        if !self.active_pane_title.is_empty() {
            left = left
                .child(text(" \u{00B7} ").color(dim))
                .child(text(self.active_pane_title.clone()).color(fg));
        }

        let mut right = div()
            .w(hints_w)
            .h(cx.cell_h)
            .flex_row()
            .items_center()
            .justify_end();
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                right = right.child(div().w(hint_spacing).h(cx.cell_h));
            }
            right = right
                .child(text(item.key.clone()).color(accent))
                .child(text(format!(" {}", item.label)).color(dim));
        }

        div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .flex_col()
                .bg(bar_bg)
                .child(div().w(rect.w).h(tokens::BORDER_THIN).bg(sep_color))
                .child(
                    div()
                        .w(rect.w)
                        .h((rect.h - tokens::BORDER_THIN).max(0.0))
                        .flex_row()
                        .items_center()
                        .px(padding)
                        .child(left)
                        .child(right),
                ),
        )
    }
}
