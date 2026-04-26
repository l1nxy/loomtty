use super::info_box::action_short_label;
use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiRect, UiScene};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, IntoElement, Layer, Render, RenderCtx, Styled, div, text};

struct HintItem {
    key: String,
    label: String,
}

pub(crate) struct HintsBarComponent {
    pane_count: usize,
    active_pane_title: String,
    hints: Vec<HintItem>,
    /// Cell metrics + total hints width frozen at `capture()` so
    /// `build_tree` doesn't need `cx.cell_w/h` or runtime
    /// `text_layout::measure` calls. Same pattern as the other widgets
    /// migrated to `Render`.
    cell_w: f32,
    cell_h: f32,
    /// Sum of `hints[i].width + cell_w*2 (gap)` for i>0 — the total
    /// width the hints column occupies, computed from the same hints
    /// vector this component holds. Only used for layout.
    hints_total_w: f32,
    /// Layout rect supplied by the host's chrome layout, captured at
    /// `capture()` time. `Render::render` reads it directly so the
    /// trait method needs no rect parameter.
    rect: UiRect,
}

impl HintsBarComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>, rect: UiRect) -> Self {
        let ws = app.core.workspaces.active();
        let pane_count = ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>();
        let active_pane_title = ws
            .active_pane_id()
            .and_then(|id| app.core.pane_grids.get(&id))
            .map(|g| g.title.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();

        // Pre-measure each hint's combined shaped width and accumulate
        // the total. Pulling this into `capture` lets `build_tree` work
        // off a minimal `RenderCtx` — without it the hint widths would
        // need re-measuring per frame against the host shaper from
        // `UiContext`, which `RenderCtx` deliberately doesn't carry.
        let hint_spacing = cx.cell_w * 2.0;
        let hints_raw = Self::pick_hints(app);
        let mut hints = Vec::with_capacity(hints_raw.len());
        let mut hints_total_w = 0.0_f32;
        for (i, (key, label)) in hints_raw.into_iter().enumerate() {
            if i > 0 {
                hints_total_w += hint_spacing;
            }
            hints_total_w += text_layout::measure(cx, &key)
                + text_layout::measure(cx, &format!(" {}", label));
            hints.push(HintItem { key, label });
        }

        Self {
            pane_count,
            active_pane_title,
            hints,
            cell_w: cx.cell_w,
            cell_h: cx.cell_h,
            hints_total_w,
            rect,
        }
    }

    /// Pick (key, label) tuples for the current state. Returned untyped
    /// so `capture()` is the single place that pre-measures shaped
    /// widths into `HintItem.width`.
    fn pick_hints(app: &App) -> Vec<(String, String)> {
        let h = |key: &str, label: &str| (key.to_string(), label.to_string());

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
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // Fifth production usage of `ciri_ui::Render`. The chrome
        // layout `rect` was captured into `self.rect` at `capture()`
        // time, so the trait method needs no extra parameter.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }

    fn build_tree(&self, rect: UiRect, cx: &RenderCtx<'_>) -> Div {
        let bar_bg = cx.theme.statusbar_bg;
        let accent = cx.theme.accent;
        let dim = cx.theme.on_surface_muted;
        let fg = cx.theme.on_surface;
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let padding = self.cell_w;

        // Pre-computed hints widths from `capture()` — clamp the cached
        // total to the layout-time max so a wide hints column can't
        // squeeze the left side off-screen.
        let hint_spacing = self.cell_w * 2.0;
        let max_hints_w = rect.w * 0.6;
        let hints_w = self.hints_total_w.min(max_hints_w);

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
            .h(self.cell_h)
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
            .h(self.cell_h)
            .flex_row()
            .items_center()
            .justify_end();
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                right = right.child(div().w(hint_spacing).h(self.cell_h));
            }
            right = right
                .child(text(item.key.clone()).color(accent))
                .child(text(format!(" {}", item.label)).color(dim));
        }

        div().w(cx.viewport[0]).h(cx.viewport[1]).child(
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

impl Render for HintsBarComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        // Layout rect is owned by `self` (set in `capture()` from the
        // host's chrome layout), so `render` reads it directly — no
        // host plumbing through the trait signature.
        self.build_tree(self.rect, cx)
    }
}
