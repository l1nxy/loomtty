use std::cell::RefCell;

use ciri_render::ui_shaper::UiTextShaper;
use unicode_width::UnicodeWidthStr;

use super::App;

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
        let session_w = measure(shaper, &format!(" {}  ", self.session_display_name()), cw);
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
        // Shape-based widths so the tabs_area_px visibility window and the
        // session label's Fixed slot both reflect real glyph advance rather
        // than the unicode-width estimate. Without this, proportional UI
        // fonts desync the `Linear` slots from the shaped text and either
        // clip the right-side zones or leave them entirely unpainted.
        let session_w = measure(shaper, &format!(" {}  ", self.session_display_name()), cw);
        let ws_label = self.workspace_indicator_label();
        let workspace_w = measure(shaper, &ws_label, cw);
        let mode_w = measure(shaper, &self.current_mode_label().0, cw);
        // tabs_area_px is the *visibility window* for tab generation: it is
        // one cell narrower than the actual Fill slot so there is always a
        // one-cell breathing-room gap between the right-most tab and the
        // workspace label. The Fill slot (= vw - session_w - workspace_w
        // - mode_w) is wider; tab snapshots constrained to tabs_area_px
        // simply leave that last cell unpopulated. See the regression test
        // `tab_area_preserves_one_cell_gap_to_workspace` in `ui/top_bar.rs`.
        let tabs_area_px = (vw - mode_w - workspace_w - cw - session_w).max(0.0);

        TopBarLayout {
            bar_y,
            bar_height,
            session_w,
            tabs_area_px,
        }
    }
}
