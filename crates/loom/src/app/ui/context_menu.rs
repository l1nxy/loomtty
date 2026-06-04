//! Right-click context menu.
//!
//! A small modal popup anchored at the click point. Rows show the menu
//! item label; enabled rows highlight on hover. Capture stores the
//! raw click coordinates; `loom_ui::anchored()` handles viewport-aware
//! placement at paint time — edge-flips when the click lands near the
//! right or bottom edge so the cursor stays at one of the menu's
//! corners instead of inside the menu.

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiContextMenuHit, UiScene, ui_hit_bounds, ui_hit_id};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{
    AnchorCorner, Div, ElevationIndex, IntoElement, Render, RenderCtx, Styled, anchored, div, text,
};

const HIT_MENU: u64 = 1;
const HIT_SCROLLBAR_THUMB: u64 = 2;
const HIT_SCROLLBAR_TRACK: u64 = 3;
const HIT_ENTRY_BASE: u64 = 1_000_000;

fn entry_hit_id(index: usize) -> u64 {
    HIT_ENTRY_BASE + index as u64
}

fn context_menu_hit_from_id(hit_id: Option<u64>) -> UiContextMenuHit {
    match hit_id {
        // Scrollbar thumb / track route to `Menu` so the click /
        // outside-click logic treats them as "inside the menu" — no
        // close, no entry selection. The mouse-press path picks them
        // up separately via `scrollbar_drag_init` to start the drag.
        Some(HIT_MENU) | Some(HIT_SCROLLBAR_THUMB) | Some(HIT_SCROLLBAR_TRACK) => {
            UiContextMenuHit::Menu
        }
        Some(id) if id >= HIT_ENTRY_BASE => UiContextMenuHit::Entry((id - HIT_ENTRY_BASE) as usize),
        _ => UiContextMenuHit::None,
    }
}

/// Whether a click at `(mx, my)` landed on the scrollbar thumb or
/// track, and how to seed a drag. Caller (mouse-press handler)
/// converts this into a `ContextMenuScrollbarDrag` placed on `App`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum ScrollbarHit {
    /// Click on the thumb — start a drag using the cursor's Y delta.
    Thumb,
    /// Click on the track above the thumb — page up by `visible_rows`.
    TrackAbove,
    /// Click on the track below the thumb — page down by `visible_rows`.
    TrackBelow,
}

struct ContextMenuRow {
    label: String,
    enabled: bool,
}

pub(crate) struct ContextMenuComponent {
    x: f32,
    y: f32,
    menu_width: f32,
    menu_height: f32,
    item_height: f32,
    rows: Vec<ContextMenuRow>,
    /// Which corner of the menu sits at `(x, y)`. Right-click menus
    /// pin their top-left to the cursor (`TopLeft`); the settings-panel
    /// enum dropdown pins its top edge centered on the trigger so the
    /// popup's midpoint matches the trigger's midpoint (`TopCenter`).
    anchor: AnchorCorner,
    /// Row offset of the topmost visible item. `0` when the menu fits
    /// entirely; clamped to `rows.len() - visible_rows` otherwise.
    scroll_offset: usize,
    /// How many item rows fit inside the (possibly capped) menu body.
    /// `rows.len()` when nothing overflows.
    visible_rows: usize,
}

impl ContextMenuComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.core.context_menu.visible {
            return None;
        }

        let padding = tokens::SPACE_2;
        let item_height = tokens::control_height_sm(cx.cell_h);
        let max_menu_width = (cx.viewport_w - padding * 2.0).max(1.0);
        // Auto-size to the widest item label so longer entries
        // (settings panel theme dropdown's full preset names) are not
        // chopped to "✓ catppuccin_…". The 200 px floor preserves the
        // historic minimum for short pane-context items
        // (Copy/Paste/Split…) so they don't render as a tiny strip.
        let widest_label = app
            .core
            .context_menu
            .items
            .iter()
            .map(|item| text_layout::measure(cx, &item.label))
            .fold(0.0_f32, f32::max);
        // Comfortable horizontal margin around the longest label —
        // `chrome_w` is the rounded panel's own padding + 1 px borders;
        // the additional `SPACE_4` is breathing room on the right so
        // labels don't kiss the panel edge or the rounded corner.
        // Floor 240 px so short pane-context items (Copy / Paste / …)
        // still read as a comfortable menu rather than a tiny strip.
        // Settings-panel dropdowns bump the floor to 280 px so the
        // popup is clearly wider than the 200 px trigger box (Zed-
        // style: the popup hangs out past both edges of the trigger).
        let chrome_w = padding * 2.0 + tokens::BORDER_THIN * 2.0;
        let floor = if app.core.settings_panel_visible {
            280.0
        } else {
            240.0
        };
        let menu_width = (widest_label + chrome_w + tokens::SPACE_4)
            .max(floor)
            .min(max_menu_width);
        // Cap the menu at ~60% of the viewport height so long lists
        // (font family picker → hundreds of entries) stay on-screen.
        // `visible_rows` is the integer item count that fits inside
        // `max_h`; the rendered tree only paints that slice and shows
        // a scrollbar on the side when there's more.
        let total_rows = app.core.context_menu.items.len();
        let max_h = (cx.viewport_h * 0.6).max(item_height * 4.0 + padding * 2.0);
        let usable_h = (max_h - padding * 2.0).max(0.0);
        let mut visible_rows = (usable_h / item_height).floor() as usize;
        if visible_rows == 0 {
            visible_rows = 1;
        }
        if visible_rows > total_rows {
            visible_rows = total_rows;
        }
        let menu_height = visible_rows as f32 * item_height + padding * 2.0;
        // Clamp the model's offset to the valid range — opening a
        // new menu resets the offset to `0` (see
        // `enter_modal_close_peers`), but a model that's stale by a
        // frame could land here with an out-of-range value.
        let max_offset = total_rows.saturating_sub(visible_rows);
        let scroll_offset = app.core.context_menu_scroll_offset.min(max_offset);
        // Capture raw click point — `anchored()` in `build_tree`
        // handles viewport-aware placement (edge-flips on the right /
        // bottom, final clamp if neither corner fits). The pre-Step-42
        // clamp baked into capture ran on the wrong side of the
        // truncation pipeline anyway; deferring it to paint-time
        // means the menu can still anchor at the cursor when there's
        // room, instead of always sliding into view.
        let x = app.core.context_menu.x;
        let y = app.core.context_menu.y;
        let label_budget = (menu_width - chrome_w).max(0.0);
        let rows = app
            .core
            .context_menu
            .items
            .iter()
            .map(|item| ContextMenuRow {
                label: text_layout::truncate_with_ellipsis(cx, &item.label, label_budget),
                enabled: item.enabled,
            })
            .collect();
        // Settings-panel dropdown → `TopCenter` (popup centered on the
        // trigger). All other context menus → `TopLeft` (cursor at the
        // popup's top-left corner, the classic right-click behaviour).
        // The settings panel is the only Base-tier modal under which a
        // context menu can be open, so the visibility flag is a clean
        // signal.
        let anchor = if app.core.settings_panel_visible {
            AnchorCorner::TopCenter
        } else {
            AnchorCorner::TopLeft
        };
        Some(Self {
            x,
            y,
            menu_width,
            menu_height,
            item_height,
            rows,
            anchor,
            scroll_offset,
            visible_rows,
        })
    }

    /// Total item count irrespective of scroll. Used by mouse-wheel
    /// handling to clamp scroll offsets.
    pub(crate) fn total_rows(&self) -> usize {
        self.rows.len()
    }

    /// Rows currently visible in the menu body (may be < total).
    pub(crate) fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    /// Scrollbar track height in pixels — `visible_rows * item_h`.
    /// Public so the mouse-press handler can seed a drag without
    /// reproducing the layout math.
    pub(crate) fn scrollbar_track_height(&self) -> f32 {
        self.visible_rows as f32 * self.item_height
    }

    /// Scrollbar thumb height — proportional to `visible / total`,
    /// floored at half a row so the thumb stays grabbable on very
    /// long lists.
    pub(crate) fn scrollbar_thumb_height(&self) -> f32 {
        let track_h = self.scrollbar_track_height();
        let total = self.rows.len().max(1) as f32;
        (track_h * (self.visible_rows as f32 / total)).max(self.item_height * 0.5)
    }

    /// Maximum legal `scroll_offset` (`total - visible`, ≥ 1 when
    /// scrollable). Returns `0` when the menu fits entirely.
    pub(crate) fn max_scroll_offset(&self) -> usize {
        self.rows.len().saturating_sub(self.visible_rows)
    }

    /// Where the mouse-press at `(mx, my)` landed within the scrollbar
    /// region — `None` if it didn't hit the bar at all. `TrackAbove`
    /// / `TrackBelow` are derived by comparing the press Y against the
    /// thumb's current screen-space midpoint (found via
    /// `ui_hit_bounds` on the thumb's hit_id).
    pub(crate) fn scrollbar_hit(
        &self,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<ScrollbarHit> {
        let render_cx = Self::render_cx(cx);
        let root = self.build_tree(&render_cx);
        match ui_hit_id(&root, cx, mx, my)? {
            HIT_SCROLLBAR_THUMB => Some(ScrollbarHit::Thumb),
            HIT_SCROLLBAR_TRACK => {
                let thumb_mid = ui_hit_bounds(&root, cx, HIT_SCROLLBAR_THUMB)
                    .map(|[_, y, _, h]| y + h * 0.5);
                match thumb_mid {
                    Some(mid) if my < mid => Some(ScrollbarHit::TrackAbove),
                    _ => Some(ScrollbarHit::TrackBelow),
                }
            }
            _ => None,
        }
    }

    /// Project loom's `UiContext` to the minimal `RenderCtx` the
    /// `Render` trait promises. Same pattern as palette: keeps the
    /// trait's context narrow and lets the host carry shaper / taffy
    /// fields out-of-band.
    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiContextMenuHit {
        let render_cx = Self::render_cx(cx);
        let root = self.build_tree(&render_cx);
        context_menu_hit_from_id(ui_hit_id(&root, cx, mx, my))
    }

    /// Hover row index resolved through the same anchored layout that
    /// paint uses. Returns `None` when the cursor is outside the menu,
    /// over a non-row region (border / padding / `HIT_MENU` background),
    /// or over a disabled row. Used by the chrome cache hash so a row-
    /// boundary crossing invalidates the cache without a stored field.
    /// Replaces the pre-Step-42 manual clamp + row-rect math, which
    /// hashed against the un-flipped rectangle and went stale whenever
    /// `anchored()` flipped the menu near the bottom-right viewport
    /// corner.
    pub(crate) fn hover_index(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<usize> {
        match self.hit_test(mx, my, cx) {
            UiContextMenuHit::Entry(idx) => {
                self.rows.get(idx).filter(|row| row.enabled).map(|_| idx)
            }
            UiContextMenuHit::Menu | UiContextMenuHit::None => None,
        }
    }
}

impl ContextMenuComponent {
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, _cx) {
            UiContextMenuHit::Entry(idx) => Some(UiAction::ExecuteContextMenuEntry(idx)),
            UiContextMenuHit::Menu => None,
            UiContextMenuHit::None => Some(UiAction::CloseContextMenu),
        }
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let padding = tokens::SPACE_2;
        let bw = tokens::BORDER_THIN;

        // Menu body sits at the `Panel` elevation tier — same recessed
        // sunk surface palette body uses, so menu and palette read as
        // tonal siblings.
        let bg_color = ElevationIndex::Panel.bg(cx.theme);
        let border_color = cx.theme.border;
        let fg_color = cx.theme.on_surface;
        let dim_color = cx.theme.on_surface_muted;
        // Neutral hover (`element_hover`, preset-independent). Items
        // here aren't selectable, so the accent has no resting state to
        // claim — keep the whole menu tonally consistent across presets.
        let hover_bg = cx.theme.element_hover;

        let content_w = self.menu_width - bw * 2.0;
        let item_h = self.item_height;
        let text_pad = (padding - bw).max(0.0);
        let scrollable = self.rows.len() > self.visible_rows;
        // Reserve room on the right for the scrollbar track when the
        // menu has more items than visible rows. The accent thumb
        // sits inside this column so it doesn't overlap the row text.
        let scrollbar_col_w = if scrollable {
            tokens::SPACE_1 * 2.0
        } else {
            0.0
        };
        let row_content_w = (content_w - scrollbar_col_w).max(0.0);
        // Panel sizes itself; positioning is delegated to `anchored()`
        // below. No `.absolute().left().top()` because the anchored
        // wrapper overrides `parent_local` at drain time based on the
        // child's measured size + viewport.
        let mut panel = div()
            .w(self.menu_width)
            .h(self.menu_height)
            .flex_col()
            .items_center()
            .bg(bg_color)
            .rounded(cx.theme.radius.md)
            .border(bw, border_color)
            // shadow_lg (was shadow_md) so the menu reads as clearly
            // floating over surfaces whose tone sits close to the
            // sunken Panel tier — top bar `statusbar_bg` (~#161514) is
            // only ~6 channel units lighter than the menu bg
            // (`surface_sunken` = #100E0C), and the previous shadow_md
            // wasn't strong enough to give the menu a visible edge
            // when right-clicked on the top bar.
            .shadow_lg()
            .hit_id(HIT_MENU)
            .child(div().w(content_w).h(padding));

        // Only render the slice of rows that fits inside the menu.
        // Absolute row indices are preserved in the hit_id so click
        // routing still maps to the right item regardless of scroll.
        let start = self.scroll_offset;
        let end = (start + self.visible_rows).min(self.rows.len());
        let mut body = div().w(content_w).flex_row();
        let mut rows_col = div().flex_col();
        for (rel_index, row) in self.rows[start..end].iter().enumerate() {
            let abs_index = start + rel_index;
            let row_el = div()
                .w(row_content_w)
                .h(item_h)
                .flex_row()
                .items_center()
                .text_color(fg_color)
                .hit_id(entry_hit_id(abs_index))
                .cursor_pointer()
                .hover(|s| s.bg(hover_bg))
                .disabled(!row.enabled, |s| s.text_color(dim_color))
                .child(div().w(text_pad).h(item_h))
                .child(text(row.label.clone()));
            rows_col = rows_col.child(row_el);
        }
        body = body.child(rows_col);

        if scrollable {
            // Scrollbar track + thumb, same shape as the command
            // palette's. Thumb height proportional to visible / total;
            // top position proportional to scroll offset within the
            // [0, max_offset] range. The track and thumb each carry a
            // dedicated hit_id so the mouse-press handler can decide
            // between "start drag" (thumb) and "page jump" (track).
            let track_w = tokens::SPACE_1;
            let track_h = self.scrollbar_track_height();
            let thumb_h = self.scrollbar_thumb_height();
            let max_offset = self.rows.len().saturating_sub(self.visible_rows).max(1);
            let thumb_top =
                (track_h - thumb_h).max(0.0) * (self.scroll_offset as f32 / max_offset as f32);
            // The thumb hit area widens to the full scrollbar column so
            // users don't have to land on the 4 px rail exactly. The
            // visible thumb still rides at `track_w` so the bar reads
            // as a slim accent.
            let track_col = div()
                .w(scrollbar_col_w)
                .h(track_h)
                .flex_row()
                .items_center()
                .justify_center()
                .hit_id(HIT_SCROLLBAR_TRACK)
                .child(
                    div()
                        .w(track_w)
                        .h(track_h)
                        .bg(tokens::tint(border_color, tokens::ALPHA_SCROLL_TRACK))
                        .child(
                            div()
                                .w(track_w)
                                .h(thumb_h)
                                .translate(0.0, thumb_top)
                                .hit_id(HIT_SCROLLBAR_THUMB)
                                .bg(tokens::tint(
                                    cx.theme.accent,
                                    tokens::ALPHA_SCROLL_THUMB,
                                )),
                        ),
                );
            body = body.child(track_col);
        }

        panel = panel.child(body).child(div().w(content_w).h(padding));

        // `anchored()` is `deferred()` with viewport-aware
        // positioning — drain reads the panel's measured size and
        // edge-flips when the click point near the bottom-right
        // would extend the menu off-screen. Replaces the manual
        // `.clamp(...)` previously applied at capture time.
        let root = div().w(cx.viewport[0]).h(cx.viewport[1]).child(anchored(
            panel,
            [self.x, self.y],
            self.anchor,
        ));

        root
    }

    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // Second production usage of `loom_ui::Render` after palette.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }
}

impl Render for ContextMenuComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::types::test_ui_context;
    use crate::app::{App, ContextMenu, ContextMenuAction, ContextMenuItem};
    use loom_config::config::LoomConfig;

    fn make_app() -> App {
        App::new(LoomConfig::default(), "test-session")
    }

    #[test]
    fn capture_records_raw_click_anchor_for_anchored_placement() {
        // Step 42 contract: `capture` stores the raw click point;
        // viewport clamping happens at paint via `anchored()`. The
        // rendered position (verified by `hit_test`) lands inside
        // the viewport even though `menu.x/y` are out-of-bounds.
        let mut app = make_app();
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 999.0,
            y: 999.0,
            target_pane_id: None,
            items: vec![ContextMenuItem {
                label: "Copy".into(),
                action: ContextMenuAction::Copy,
                enabled: true,
            }],
        };
        let mut config = LoomConfig::default();
        config.theme = app.core.config.theme.clone();
        let theme = loom_ui::ResolvedTheme::default();
        let cx = test_ui_context(&config, &theme, 120.0, 60.0);
        let menu = ContextMenuComponent::capture(&app, &cx).expect("menu visible");
        // Capture stores raw click coords — no pre-paint clamp.
        assert!((menu.x - 999.0).abs() < 0.001);
        assert!((menu.y - 999.0).abs() < 0.001);
        // The menu still has a well-defined size; anchored uses these
        // dimensions during drain to compute the on-screen bounds.
        assert!(menu.menu_width > 0.0);
        assert!(menu.menu_height > 0.0);
    }

    #[test]
    fn hit_test_uses_loom_ui_layout_snapshot() {
        let mut app = make_app();
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 40.0,
            y: 50.0,
            target_pane_id: None,
            items: vec![
                ContextMenuItem {
                    label: "Copy".into(),
                    action: ContextMenuAction::Copy,
                    enabled: true,
                },
                ContextMenuItem {
                    label: "Paste".into(),
                    action: ContextMenuAction::Paste,
                    enabled: false,
                },
            ],
        };
        let cx = app.ui_context();
        let menu = ContextMenuComponent::capture(&app, &cx).expect("menu visible");

        assert_eq!(menu.hit_test(60.0, 65.0, &cx), UiContextMenuHit::Entry(0));
        assert_eq!(menu.hit_test(60.0, 90.0, &cx), UiContextMenuHit::Menu);
        assert_eq!(menu.hit_test(10.0, 10.0, &cx), UiContextMenuHit::None);
    }
}
