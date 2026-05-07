use std::cell::RefCell;

use ciri_config::config::StatusBarSegmentKind;
use ciri_render::ui_shaper::UiTextShaper;
use unicode_width::UnicodeWidthStr;

use super::App;
use super::ui::tokens::SEGMENT_PAD_X;

/// Wrap a measured text width with the section's text padding so
/// session / workspace / mode all share the same lualine-style
/// flat-rectangle geometry. Returns 0 for an empty input so the
/// workspace slot collapses to nothing when there is no label.
pub(crate) fn segment_slot_width(text_w: f32) -> f32 {
    if text_w <= 0.0 {
        0.0
    } else {
        text_w + 2.0 * SEGMENT_PAD_X
    }
}

/// Per-segment sizing intent. Fixed segments measure to a preferred width
/// from their cached label / contents; the Fill segment gobbles whatever
/// horizontal space is left after fixed segments are allocated.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SegmentMeasure {
    Fixed(f32),
    Fill,
}

/// Drop-priority for a segment kind when the bar is too narrow to fit
/// every fixed segment at its preferred width. *Lower number = drops
/// width first.* Fill segments are not capped by priority — they're
/// allocated last from whatever fixed segments leave behind.
///
/// The historical behaviour was `mode > session > workspace`; usage
/// inherits workspace's tier because both are "redundant context" that
/// can collapse on narrow viewports.
pub(crate) fn segment_priority(kind: StatusBarSegmentKind) -> u8 {
    match kind {
        StatusBarSegmentKind::Mode => 30,
        StatusBarSegmentKind::SessionLabel => 20,
        StatusBarSegmentKind::Workspace => 10,
        StatusBarSegmentKind::Usage => 10,
        StatusBarSegmentKind::PaneTabs => 0, // Fill — never capped here.
    }
}

/// Allocate per-segment widths to fit `bar_w`, respecting priority for
/// fixed segments and splitting the remainder across fill segments.
///
/// Returns one width per input segment in the same order. Fixed
/// segments at higher priority keep their preferred width first; lower
/// priorities trim. Fill segments share whatever's left, evenly. If
/// even after trimming all fixed widths we'd still overflow, fill
/// segments collapse to zero (they always non-negative).
pub(crate) fn cap_segments(
    bar_w: f32,
    segments: &[(StatusBarSegmentKind, SegmentMeasure)],
) -> Vec<f32> {
    let bar_w = bar_w.max(0.0);

    // Sort fixed segments by descending priority so we honour Mode > Session > rest.
    let mut fixed_idxs: Vec<usize> = segments
        .iter()
        .enumerate()
        .filter_map(|(i, (_, m))| matches!(m, SegmentMeasure::Fixed(_)).then_some(i))
        .collect();
    fixed_idxs.sort_by(|a, b| {
        segment_priority(segments[*b].0).cmp(&segment_priority(segments[*a].0))
    });

    let mut widths = vec![0.0_f32; segments.len()];
    let mut remaining = bar_w;
    for &i in &fixed_idxs {
        let SegmentMeasure::Fixed(want) = segments[i].1 else {
            unreachable!()
        };
        let alloc = want.max(0.0).min(remaining);
        widths[i] = alloc;
        remaining -= alloc;
    }

    let fill_count = segments
        .iter()
        .filter(|(_, m)| matches!(m, SegmentMeasure::Fill))
        .count();
    if fill_count > 0 {
        let per = (remaining / fill_count as f32).max(0.0);
        for (i, (_, m)) in segments.iter().enumerate() {
            if matches!(m, SegmentMeasure::Fill) {
                widths[i] = per;
            }
        }
    }

    widths
}

/// Compatibility shim for the historical 4-slot capper. New code goes
/// through `cap_segments` against the configured segment list.
pub(crate) fn cap_fixed_section_widths(
    bar_w: f32,
    session_w: f32,
    workspace_w: f32,
    mode_w: f32,
) -> (f32, f32, f32) {
    let segs = [
        (
            StatusBarSegmentKind::SessionLabel,
            SegmentMeasure::Fixed(session_w),
        ),
        (StatusBarSegmentKind::PaneTabs, SegmentMeasure::Fill),
        (
            StatusBarSegmentKind::Workspace,
            SegmentMeasure::Fixed(workspace_w),
        ),
        (
            StatusBarSegmentKind::Mode,
            SegmentMeasure::Fixed(mode_w),
        ),
    ];
    let widths = cap_segments(bar_w, &segs);
    (widths[0], widths[2], widths[3])
}

/// Shape-aware pixel width with a cell-grid fallback. Kept here (rather
/// than pulling `ui::text_layout` into the non-UI `top_bar` module) so
/// layout math stays consistent whether measured from UI components (which
/// have a `UiContext`) or from the chrome layer (which only has a shaper
/// handle threaded through from the renderer).
pub(crate) fn measure(shaper: Option<&RefCell<UiTextShaper>>, text: &str, cell_w: f32) -> f32 {
    if let Some(cell) = shaper {
        let mut s = cell.borrow_mut();
        if s.has_face() {
            return s.measure(text);
        }
    }
    UnicodeWidthStr::width(text) as f32 * cell_w
}

/// Reference character used to size pane-tab slots in UI-font advance
/// units. Width is `config.tabbar.pane_tab_width_chars` × `measure(REF)`,
/// so an `N`-char label rendered in the UI font actually fills the tab
/// rather than occupying a fraction of a terminal-cell-sized slot. `'0'`
/// is a reasonable proxy for typical label content — most UI fonts make
/// digits tabular and close to the average alphabetic-glyph advance.
const TAB_SLOT_REF_CHAR: &str = "0";

#[derive(Debug, Clone)]
pub(crate) struct PaneTabLayout {
    pub pane_id: u64,
    pub label: String,
    pub x: f32,
    pub w: f32,
    pub active: bool,
}

/// Per-frame layout numbers needed by `TopBarComponent` *before* the
/// `Linear` row computes its own slots.
///
/// We keep `bar_y` / `bar_height` / `session_w` / `tabs_area_px` because:
/// - `bar_y` / `bar_height` are needed to position the bar rect itself
///   (fed into `UiElement::paint(rect, ...)`).
/// - `session_w` is the only fixed-zone width the row needs that depends
///   on session data (length of session display name).
/// - `tabs_area_px` is the (one-cell narrower) tab visibility window
///   used by `pane_tab_layouts()` and `ensure_active_pane_tab_visible()`.
///
/// Per-zone absolute x coordinates (`session_x`, `workspace_x`, `mode_x`)
/// were intentionally removed in the tree-layout refactor — those are now
/// computed lazily by `Linear::layout` at paint/hit time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TopBarLayout {
    pub bar_y: f32,
    pub bar_height: f32,
    pub session_w: f32,
    pub tabs_area_px: f32,
}

impl App {
    pub(crate) fn hit_test_top_bar(&self, mx: f32, my: f32) -> bool {
        let (cell_w, cell_h) = self.cell_dimensions();
        // `bar_y`/`bar_height` depend only on cell metrics, not on shaped
        // text widths — a `None` shaper here just falls back to the
        // cell-grid estimate for `session_w`/`tabs_area_px`, which we
        // ignore anyway.
        let layout = self.top_bar_layout(
            self.core.workspaces.view_size.width,
            self.core.workspaces.view_size.height + self.total_chrome_height(),
            cell_w,
            cell_h,
            None,
        );
        mx >= 0.0 && my >= layout.bar_y && my <= layout.bar_y + layout.bar_height
    }

    pub(crate) fn ensure_active_pane_tab_visible(&mut self, tab_area_px: f32) {
        if tab_area_px <= 0.0 {
            self.pane_tab_scroll = 0.0;
            return;
        }

        let tab_count = self.pane_tab_entries().len();
        if tab_count == 0 {
            self.pane_tab_scroll = 0.0;
            return;
        }

        let cw = self.cell_dimensions().0;
        let tab_w = self.pane_tab_slot_width(cw, self.ui_shaper.as_ref());
        let total_w = tab_count as f32 * tab_w;
        let max_scroll = (total_w - tab_area_px).max(0.0);

        let active_pane = self.core.workspaces.active().active_pane_id();
        let active_idx = self
            .pane_tab_entries()
            .iter()
            .position(|(id, _)| Some(*id) == active_pane)
            .unwrap_or(0);
        let active_start = active_idx as f32 * tab_w;
        let active_end = active_start + tab_w;
        let mut scroll = self.pane_tab_scroll.clamp(0.0, max_scroll);
        if active_start < scroll {
            scroll = active_start;
        } else if active_end > scroll + tab_area_px {
            scroll = (active_end - tab_area_px).max(0.0);
        }
        self.pane_tab_scroll = scroll.clamp(0.0, max_scroll);
    }

    pub(crate) fn pane_tab_scroll_max(&self) -> f32 {
        // When tabs are extracted into a dedicated side bar, the top bar
        // holds no pane tabs, so the horizontal scroll concept is
        // meaningless. Returning 0.0 early also prevents stale scroll
        // state from leaking across `hit_test_top_bar`-gated mouse-wheel
        // events (which clamp `App.pane_tab_scroll` against this max)
        // and from flapping the render-snapshot hash that uses this
        // value as a cache-key input.
        if !matches!(
            self.core.config.tabbar.position,
            ciri_config::config::TabBarPosition::Integrated,
        ) {
            return 0.0;
        }
        let ch = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let cw = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_width)
            .unwrap_or(8.0);
        let tab_area_px = self
            .top_bar_layout(
                self.core.workspaces.view_size.width,
                self.core.workspaces.view_size.height + self.total_chrome_height(),
                cw,
                ch,
                self.ui_shaper.as_ref(),
            )
            .tabs_area_px;

        let tab_count = self.pane_tab_entries().len() as f32;
        let tab_w = self.pane_tab_slot_width(cw, self.ui_shaper.as_ref());
        let total_w = tab_count * tab_w;
        (total_w - tab_area_px).max(0.0)
    }

    /// Fixed per-tab slot width in UI-font-advance units.
    ///
    /// Returns `pane_tab_width_chars × measure(shaper, TAB_SLOT_REF_CHAR)`
    /// when the UI shaper has a face loaded, falling back to the legacy
    /// `N × terminal_cell_w` when it doesn't (tests, pre-init). Called from
    /// all three callers that compute pane-tab geometry so they stay in
    /// sync — the `render_snapshot_hash` caching in `render.rs` depends on
    /// `pane_tab_scroll_max` agreeing with what `pane_tab_layouts` paints.
    fn pane_tab_slot_width(&self, cw: f32, shaper: Option<&RefCell<UiTextShaper>>) -> f32 {
        let n = self.core.config.tabbar.pane_tab_width_chars;
        measure(shaper, &TAB_SLOT_REF_CHAR.repeat(n), cw)
    }

    pub(crate) fn pane_tab_layouts(
        &self,
        cw: f32,
        tabs_area_px: f32,
        shaper: Option<&RefCell<UiTextShaper>>,
    ) -> Vec<PaneTabLayout> {
        let tab_w = self.pane_tab_slot_width(cw, shaper);
        let session_w = segment_slot_width(measure(shaper, &self.session_display_name(), cw));
        // NB: tab `x` coordinates are absolute screen coordinates assuming the
        // top bar starts at screen x=0. This holds for the current
        // `Border { top | bottom }` chrome configurations (no `left`/`right`
        // edges).
        let tabs_start_x = session_w;
        let tabs_end_x = tabs_start_x + tabs_area_px;
        let mut x = tabs_start_x - self.pane_tab_scroll;
        let mut layouts = Vec::new();

        for (idx, (pane_id, title)) in self.pane_tab_entries().into_iter().enumerate() {
            let label = self.format_pane_tab_label(idx, &title);
            let active = self.core.workspaces.active().active_pane_id() == Some(pane_id);
            let tab_x = x;
            let tab_end = tab_x + tab_w;
            if tab_end > tabs_start_x && tab_x < tabs_end_x {
                layouts.push(PaneTabLayout {
                    pane_id,
                    label,
                    x: tab_x,
                    w: tab_w,
                    active,
                });
            }
            x += tab_w;
        }

        layouts
    }

    /// Delegate: get pane tab entries.
    pub(crate) fn pane_tab_entries(&self) -> Vec<(u64, String)> {
        self.core.pane_tab_entries()
    }

    /// Delegate: format pane tab label.
    pub(crate) fn format_pane_tab_label(&self, idx: usize, title: &str) -> String {
        self.core.format_pane_tab_label(idx, title)
    }

    /// Delegate: session display name (includes remote host if applicable).
    pub(crate) fn session_display_name(&self) -> String {
        self.core.session_display_name()
    }

    /// Delegate: workspace indicator label.
    pub(crate) fn workspace_indicator_label(&self) -> String {
        self.core.workspace_indicator_label()
    }

    /// Delegate: current mode label.
    pub(crate) fn current_mode_label(&self) -> (String, [f32; 4]) {
        self.core.current_mode_label()
    }

    pub(crate) fn top_bar_layout(
        &self,
        vw: f32,
        vh: f32,
        cw: f32,
        ch: f32,
        shaper: Option<&RefCell<UiTextShaper>>,
    ) -> TopBarLayout {
        let padding = if let Some(px) = self.core.config.statusbar.height_padding {
            px
        } else {
            ch * self.core.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        let bar_y = self.status_bar_y(vh);

        let segs = self.top_bar_segments(vw, cw, shaper);
        // Legacy field accessors: pull session / pane-tabs widths out of
        // the configured segment list so callers that still ask for
        // `session_w` / `tabs_area_px` get the right number even when
        // the user reorders or omits these segments.
        let session_w = segs
            .iter()
            .find_map(|(k, w)| (*k == StatusBarSegmentKind::SessionLabel).then_some(*w))
            .unwrap_or(0.0);
        let tabs_area_px = segs
            .iter()
            .find_map(|(k, w)| (*k == StatusBarSegmentKind::PaneTabs).then_some(*w))
            .unwrap_or(0.0);

        TopBarLayout {
            bar_y,
            bar_height,
            session_w,
            tabs_area_px,
        }
    }

    /// Compute the configured segment list with allocated widths. Used
    /// by `top_bar_layout` for legacy fields and directly by
    /// `TopBarComponent::row_slots` for the actual slot rects.
    pub(crate) fn top_bar_segments(
        &self,
        vw: f32,
        cw: f32,
        shaper: Option<&RefCell<UiTextShaper>>,
    ) -> Vec<(StatusBarSegmentKind, f32)> {
        let kinds = self.core.config.statusbar.effective_segments();
        let measures: Vec<(StatusBarSegmentKind, SegmentMeasure)> = kinds
            .iter()
            .map(|k| (*k, self.measure_top_bar_segment(*k, cw, shaper)))
            .collect();
        let widths = cap_segments(vw, &measures);
        kinds.into_iter().zip(widths).collect()
    }

    /// Preferred sizing intent for one segment kind. Fixed segments
    /// measure their own cached label; PaneTabs is the bar's flex zone.
    pub(crate) fn measure_top_bar_segment(
        &self,
        kind: StatusBarSegmentKind,
        cw: f32,
        shaper: Option<&RefCell<UiTextShaper>>,
    ) -> SegmentMeasure {
        match kind {
            StatusBarSegmentKind::SessionLabel => SegmentMeasure::Fixed(segment_slot_width(
                measure(shaper, &self.session_display_name(), cw),
            )),
            StatusBarSegmentKind::PaneTabs => SegmentMeasure::Fill,
            StatusBarSegmentKind::Workspace => {
                let label = self.workspace_indicator_label();
                SegmentMeasure::Fixed(segment_slot_width(measure(shaper, &label, cw)))
            }
            StatusBarSegmentKind::Mode => SegmentMeasure::Fixed(segment_slot_width(measure(
                shaper,
                &self.current_mode_label().0,
                cw,
            ))),
            StatusBarSegmentKind::Usage => {
                let label = self.usage_segment_label();
                SegmentMeasure::Fixed(segment_slot_width(measure(shaper, &label, cw)))
            }
        }
    }

    /// Plain-string label for the usage segment. Pulled here so
    /// `measure_top_bar_segment` can compute width without instantiating
    /// the renderer. The renderer (`super::ui::top_bar::usage`)
    /// reproduces the same logic for paint.
    pub(crate) fn usage_segment_label(&self) -> String {
        let snapshot = self.usage_snapshot();
        super::ui::top_bar::usage::format_label(&snapshot)
    }

    /// Hook for the segment renderer to read the latest poll. Returns
    /// the current snapshot if the App owns one, or an empty default
    /// (so the segment shows `"usage --"` placeholder text).
    pub(crate) fn usage_snapshot(&self) -> super::usage::UsageSnapshot {
        self.usage
            .as_ref()
            .map(|s| s.read())
            .unwrap_or_default()
    }
}
