//! Right-click context menu.
//!
//! A small modal popup anchored at the click point. Rows show the menu
//! item label; enabled rows highlight on hover. Capture stores the
//! raw click coordinates; `ciri_ui::anchored()` handles viewport-aware
//! placement at paint time — edge-flips when the click lands near the
//! right or bottom edge so the cursor stays at one of the menu's
//! corners instead of inside the menu.

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiContextMenuHit, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{
    AnchorCorner, Div, ElevationIndex, IntoElement, Render, RenderCtx, Styled, anchored, div, text,
};

const HIT_MENU: u64 = 1;
const HIT_ENTRY_BASE: u64 = 1_000_000;

fn entry_hit_id(index: usize) -> u64 {
    HIT_ENTRY_BASE + index as u64
}

fn context_menu_hit_from_id(hit_id: Option<u64>) -> UiContextMenuHit {
    match hit_id {
        Some(HIT_MENU) => UiContextMenuHit::Menu,
        Some(id) if id >= HIT_ENTRY_BASE => UiContextMenuHit::Entry((id - HIT_ENTRY_BASE) as usize),
        _ => UiContextMenuHit::None,
    }
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
        let chrome_w = padding * 2.0 + tokens::BORDER_THIN * 2.0;
        let menu_width = (widest_label + chrome_w + tokens::SPACE_4)
            .max(240.0)
            .min(max_menu_width);
        let menu_height = app.core.context_menu.items.len() as f32 * item_height + padding * 2.0;
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
        Some(Self {
            x,
            y,
            menu_width,
            menu_height,
            item_height,
            rows,
        })
    }

    /// Project ciri's `UiContext` to the minimal `RenderCtx` the
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
            UiContextMenuHit::Entry(idx) => self
                .rows
                .get(idx)
                .filter(|row| row.enabled)
                .map(|_| idx),
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

        for (index, row) in self.rows.iter().enumerate() {
            // Single declarative branch: every row carries hit_id +
            // cursor + hover, then `.disabled(!row.enabled, ...)`
            // clears hit_id + cursor and overlays the dim text colour
            // when the row is disabled. The Text node has no
            // `.color()` so it inherits from whichever style wins
            // (base `fg_color`, or disabled refinement `dim_color`)
            // via the walker's refinement-aware text-color thread.
            let row_el = div()
                .w(content_w)
                .h(item_h)
                .flex_row()
                .items_center()
                .text_color(fg_color)
                .hit_id(entry_hit_id(index))
                .cursor_pointer()
                .hover(|s| s.bg(hover_bg))
                .disabled(!row.enabled, |s| s.text_color(dim_color))
                .child(div().w(text_pad).h(item_h))
                .child(text(row.label.clone()));
            panel = panel.child(row_el);
        }
        panel = panel.child(div().w(content_w).h(padding));

        // `anchored()` is `deferred()` with viewport-aware
        // positioning — drain reads the panel's measured size and
        // edge-flips when the click point near the bottom-right
        // would extend the menu off-screen. Replaces the manual
        // `.clamp(...)` previously applied at capture time.
        let root = div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(anchored(
                panel,
                [self.x, self.y],
                AnchorCorner::TopLeft,
            ));

        root
    }

    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // Second production usage of `ciri_ui::Render` after palette.
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
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
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
        let mut config = CiriConfig::default();
        config.theme = app.core.config.theme.clone();
        let theme = ciri_ui::ResolvedTheme::default();
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
    fn hit_test_uses_ciri_ui_layout_snapshot() {
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
