//! Right-click context menu.
//!
//! A small modal popup anchored at the click point. Rows show the
//! menu item label; enabled rows highlight on hover. Capture clamps
//! the anchor point so the menu stays inside the viewport, so the
//! paint path just needs to position a styled panel at `(x, y)` with
//! the rows flex-stacked inside.

use ciri_ui::color::{scale_rgb, TRANSPARENT};
use ciri_ui::{div, text, Div, Layer, ResolvedTheme, Styled};

use super::tokens;
use super::types::{UiAction, UiContext, UiContextMenuHit};
use crate::app::App;

struct ContextMenuRow {
    label: String,
    enabled: bool,
    hovered: bool,
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
        let item_height = cx.cell_h * 1.5;
        let padding = 8.0;
        let menu_width = 200.0;
        let menu_height = app.core.context_menu.items.len() as f32 * item_height + padding * 2.0;
        let x = app.core.context_menu.x.min(cx.viewport_w - menu_width);
        let y = app.core.context_menu.y.min(cx.viewport_h - menu_height);
        let rows = app
            .core
            .context_menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| ContextMenuRow {
                label: item.label.clone(),
                enabled: item.enabled,
                hovered: Some(i) == app.core.context_menu.hovered_index && item.enabled,
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

    pub(super) fn hit_test(&self, mx: f32, my: f32) -> UiContextMenuHit {
        if mx < self.x
            || mx > self.x + self.menu_width
            || my < self.y
            || my > self.y + self.menu_height
        {
            return UiContextMenuHit::None;
        }
        let padding = 8.0;
        let relative_y = my - self.y - padding;
        if relative_y < 0.0 {
            return UiContextMenuHit::Menu;
        }
        let index = (relative_y / self.item_height) as usize;
        if index < self.rows.len() && self.rows[index].enabled {
            UiContextMenuHit::Entry(index)
        } else {
            UiContextMenuHit::Menu
        }
    }

    /// Map a click to a `UiAction`. Preserves the legacy semantics:
    /// clicking an enabled entry fires it, clicking on disabled rows
    /// or the panel padding is a no-op, and clicking outside closes
    /// the menu.
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiContextMenuHit::Entry(idx) => Some(UiAction::ExecuteContextMenuEntry(idx)),
            UiContextMenuHit::Menu => None,
            UiContextMenuHit::None => Some(UiAction::CloseContextMenu),
        }
    }
}

impl ContextMenuComponent {
    /// Build the ciri-ui tree for this captured menu snapshot.
    ///
    /// The root is an absolutely-positioned `Modal`-layer panel at
    /// `(self.x, self.y)` driven by `translate` — menus are small
    /// and anchored exactly where the user right-clicked, so
    /// flex-based centring doesn't apply here the way it does for
    /// the paste dialog.
    pub(crate) fn build_tree(&self, theme: &ResolvedTheme) -> Div {
        // Legacy recipe: `background * 0.9` — one step darker than the
        // terminal background so the menu reads as a raised surface
        // against the pane content.
        let bg_color = scale_rgb(theme.term_bg, 0.9);
        let hover_bg = ciri_ui::color::with_alpha(theme.accent, tokens::ALPHA_HOVER_BG);

        let padding = tokens::SPACE_2;

        let mut panel = div()
            .in_layer(Layer::Modal)
            .translate(self.x, self.y)
            .w(self.menu_width)
            .h(self.menu_height)
            .flex_col()
            .bg(bg_color)
            .border(tokens::BORDER_THIN, theme.border_focus)
            .shadow_md()
            .pt(padding)
            .pb(padding);

        for row in &self.rows {
            let text_color = if row.enabled {
                theme.on_surface
            } else {
                theme.on_surface_muted
            };
            let row_bg = if row.hovered { hover_bg } else { TRANSPARENT };
            panel = panel.child(
                div()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .h(self.item_height)
                    .px(padding)
                    .bg(row_bg)
                    .child(text(&row.label).color(text_color)),
            );
        }
        panel
    }
}
