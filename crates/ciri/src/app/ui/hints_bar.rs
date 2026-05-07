use ciri_config::config::HintsBarSegmentKind;

use super::info_box::action_short_label;
use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiRect, UiScene};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Color, Div, IntoElement, Render, RenderCtx, Styled, div, text};

struct HintItem {
    key: String,
    label: String,
}

/// Per-segment captured data. Each variant carries everything `build_tree`
/// needs — including the pre-measured natural width — so paint runs
/// without touching the host shaper.
enum SegmentData {
    PaneInfo {
        pane_count: usize,
        active_pane_title: String,
        measured_w: f32,
    },
    Usage {
        label: String,
        measured_w: f32,
    },
    LeaderHints {
        items: Vec<HintItem>,
        /// Pre-summed width of all hint key+label pairs plus inter-item gaps.
        measured_w: f32,
    },
}

impl SegmentData {
    fn kind(&self) -> HintsBarSegmentKind {
        match self {
            Self::PaneInfo { .. } => HintsBarSegmentKind::PaneInfo,
            Self::Usage { .. } => HintsBarSegmentKind::Usage,
            Self::LeaderHints { .. } => HintsBarSegmentKind::LeaderHints,
        }
    }

    fn measured_w(&self) -> f32 {
        match self {
            Self::PaneInfo { measured_w, .. }
            | Self::Usage { measured_w, .. }
            | Self::LeaderHints { measured_w, .. } => *measured_w,
        }
    }

    /// Whether this segment should soak up leftover horizontal space.
    /// PaneInfo is the row's flex zone — others stay at their natural widths.
    fn is_fill(&self) -> bool {
        matches!(self, Self::PaneInfo { .. })
    }

    /// Empty Usage / LeaderHints slots collapse so they don't insert
    /// stray separators in an otherwise quiet bar.
    fn is_empty(&self) -> bool {
        match self {
            Self::PaneInfo { .. } => false,
            Self::Usage { label, .. } => label.is_empty(),
            Self::LeaderHints { items, .. } => items.is_empty(),
        }
    }
}

pub(crate) struct HintsBarComponent {
    segments: Vec<SegmentData>,
    cell_w: f32,
    cell_h: f32,
    rect: UiRect,
}

impl HintsBarComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>, rect: UiRect) -> Self {
        let kinds = app.core.config.hints_bar.effective_segments();
        let segments = kinds
            .into_iter()
            .map(|k| Self::capture_segment(app, cx, k))
            .collect();

        Self {
            segments,
            cell_w: cx.cell_w,
            cell_h: cx.cell_h,
            rect,
        }
    }

    fn capture_segment(
        app: &App,
        cx: &UiContext<'_>,
        kind: HintsBarSegmentKind,
    ) -> SegmentData {
        match kind {
            HintsBarSegmentKind::PaneInfo => {
                let ws = app.core.workspaces.active();
                let pane_count = ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>();
                let active_pane_title = ws
                    .active_pane_id()
                    .and_then(|id| app.core.pane_grids.get(&id))
                    .map(|g| g.title.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                // Pane info auto-fills, so the measured width is only used
                // to honour a minimum (so the segment never disappears
                // entirely).
                let pane_text = if pane_count == 1 {
                    "1 pane".to_string()
                } else {
                    format!("{pane_count} panes")
                };
                let mut measured_w = text_layout::measure(cx, "● ")
                    + text_layout::measure(cx, &pane_text);
                if !active_pane_title.is_empty() {
                    measured_w += text_layout::measure(cx, " · ")
                        + text_layout::measure(cx, &active_pane_title);
                }
                SegmentData::PaneInfo {
                    pane_count,
                    active_pane_title,
                    measured_w,
                }
            }
            HintsBarSegmentKind::Usage => {
                let (label, measured_w) = if !app.usage_enabled() {
                    (String::new(), 0.0)
                } else {
                    // Lua plugin owns the visibility / formatting
                    // logic. The default `builtin:usage.lua` shows the
                    // segment only when the focused pane's title
                    // matches `claude` / `codex`; user plugins can
                    // override by registering their own `format-usage`
                    // handler. `None` here means "hide" — fall back
                    // to the Rust formatter only if no engine is up
                    // (so a config error can't blank the segment).
                    let label = match app.plugin.as_ref() {
                        Some(engine) => {
                            let ctx = app.build_usage_plugin_ctx();
                            engine.format_usage(&ctx).unwrap_or_default()
                        }
                        None => crate::app::ui::top_bar::usage::format_label(
                            &app.usage_snapshot(),
                        ),
                    };
                    let w = if label.is_empty() {
                        0.0
                    } else {
                        text_layout::measure(cx, &label)
                    };
                    (label, w)
                };
                SegmentData::Usage { label, measured_w }
            }
            HintsBarSegmentKind::LeaderHints => {
                let hint_spacing = cx.cell_w * 2.0;
                let raw = pick_leader_hints(app);
                let mut items = Vec::with_capacity(raw.len());
                let mut measured_w = 0.0_f32;
                for (i, (key, label)) in raw.into_iter().enumerate() {
                    if i > 0 {
                        measured_w += hint_spacing;
                    }
                    measured_w += text_layout::measure(cx, &key)
                        + text_layout::measure(cx, &format!(" {label}"));
                    items.push(HintItem { key, label });
                }
                SegmentData::LeaderHints { items, measured_w }
            }
        }
    }
}

/// Pick (key, label) tuples for the leader-hints segment based on
/// the current input mode. Pulled out as a free function — the data
/// has no dependencies on widget state.
fn pick_leader_hints(app: &App) -> Vec<(String, String)> {
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

impl HintsBarComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
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

    /// Allocate per-segment widths. Mirrors the top-bar capper:
    /// fixed segments take their measured widths first (capped at 60%
    /// of the bar each so a very long Usage label can't squeeze
    /// LeaderHints out), the fill segment splits whatever's left.
    /// Empty segments collapse to zero.
    fn slot_widths(&self, bar_w: f32, padding: f32) -> Vec<f32> {
        let inner_w = (bar_w - padding * 2.0).max(0.0);
        let max_fixed = inner_w * 0.6;

        let mut widths = vec![0.0_f32; self.segments.len()];
        let mut remaining = inner_w;
        // Fixed segments first.
        for (i, seg) in self.segments.iter().enumerate() {
            if seg.is_fill() || seg.is_empty() {
                continue;
            }
            let want = seg.measured_w().min(max_fixed);
            let alloc = want.min(remaining);
            widths[i] = alloc;
            remaining -= alloc;
        }
        // Fill segments split the remainder evenly.
        let fill_count = self
            .segments
            .iter()
            .filter(|s| s.is_fill() && !s.is_empty())
            .count();
        if fill_count > 0 {
            let per = (remaining / fill_count as f32).max(0.0);
            for (i, seg) in self.segments.iter().enumerate() {
                if seg.is_fill() && !seg.is_empty() {
                    widths[i] = per;
                }
            }
        }
        widths
    }

    fn build_tree(&self, rect: UiRect, cx: &RenderCtx<'_>) -> Div {
        let bar_bg = cx.theme.statusbar_bg;
        let dim = cx.theme.on_surface_muted;
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let padding = self.cell_w;
        let widths = self.slot_widths(rect.w, padding);

        // `flex_row` body that hosts each non-empty segment as a child
        // div, with thin vertical separator divs between them. Skips
        // empty segments entirely so a disabled Usage doesn't leave a
        // stray separator.
        let mut row = div()
            .w(rect.w)
            .h((rect.h - tokens::BORDER_THIN).max(0.0))
            .flex_row()
            .items_center()
            .px(padding);

        let mut painted_any = false;
        let separator_inset = tokens::SPACE_1;
        let separator_h = (self.cell_h - separator_inset * 2.0).max(0.0);
        for (i, seg) in self.segments.iter().enumerate() {
            if seg.is_empty() {
                continue;
            }
            if painted_any {
                row = row.child(separator_div(
                    self.cell_w,
                    separator_h,
                    separator_inset,
                    sep_color,
                ));
            }
            let slot_w = widths[i];
            row = row.child(self.build_segment_div(seg, slot_w, cx));
            painted_any = true;
            // _i is used implicitly via index alignment with `widths`.
        }

        div().w(cx.viewport[0]).h(cx.viewport[1]).child(
            div()
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .flex_col()
                .bg(bar_bg)
                .child(div().w(rect.w).h(tokens::BORDER_THIN).bg(sep_color))
                .child(row),
        )
    }

    fn build_segment_div(&self, seg: &SegmentData, slot_w: f32, cx: &RenderCtx<'_>) -> Div {
        let accent = cx.theme.accent;
        let dim = cx.theme.on_surface_muted;
        let fg = cx.theme.on_surface;
        let mut slot = div().w(slot_w).h(self.cell_h).flex_row().items_center();
        match seg {
            SegmentData::PaneInfo {
                pane_count,
                active_pane_title,
                ..
            } => {
                let pane_text = if *pane_count == 1 {
                    "1 pane".to_string()
                } else {
                    format!("{pane_count} panes")
                };
                slot = slot
                    .child(text("\u{25CF} ").color(accent))
                    .child(text(pane_text).color(dim));
                if !active_pane_title.is_empty() {
                    slot = slot
                        .child(text(" \u{00B7} ").color(dim))
                        .child(text(active_pane_title.clone()).color(fg));
                }
                slot
            }
            SegmentData::Usage { label, .. } => slot
                .justify_center()
                .child(text(label.clone()).color(dim)),
            SegmentData::LeaderHints { items, .. } => {
                slot = slot.justify_end();
                let hint_spacing = self.cell_w * 2.0;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        slot = slot.child(div().w(hint_spacing).h(self.cell_h));
                    }
                    slot = slot
                        .child(text(item.key.clone()).color(accent))
                        .child(text(format!(" {}", item.label)).color(dim));
                }
                slot
            }
        }
    }
}

fn separator_div(cell_w: f32, h: f32, inset: f32, color: Color) -> Div {
    // Wraps a thin vertical line in a column flex so the inset padding
    // sits above and below the line itself; the wrapper width is one
    // cell so it reads as a normal column gap rather than a hairline
    // crammed against its neighbours.
    div()
        .w(cell_w)
        .h(inset + h + inset)
        .flex_col()
        .items_center()
        .child(div().h(inset))
        .child(div().w(tokens::BORDER_THIN).h(h).bg(color))
        .child(div().h(inset))
}

impl Render for HintsBarComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(self.rect, cx)
    }
}

/// Used by tests in this crate that need to confirm the configured
/// kinds line up with the captured data.
#[cfg(test)]
impl HintsBarComponent {
    pub(crate) fn segment_kinds(&self) -> Vec<HintsBarSegmentKind> {
        self.segments.iter().map(SegmentData::kind).collect()
    }
}
