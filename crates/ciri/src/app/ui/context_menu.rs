//! Right-click context menu.
//!
//! A small modal popup anchored at the click point. Rows show the menu item
//! label; enabled rows highlight on hover. Capture clamps the menu inside the
//! viewport so painting can stay purely absolute.

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiContextMenuHit, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

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

        let padding = tokens::SPACE_2;
        let item_height = tokens::control_height_sm(cx.cell_h);
        let max_menu_width = (cx.viewport_w - padding * 2.0).max(1.0);
        let menu_width = 200.0_f32.min(max_menu_width);
        let menu_height = app.core.context_menu.items.len() as f32 * item_height + padding * 2.0;
        let x = app
            .core
            .context_menu
            .x
            .clamp(0.0, (cx.viewport_w - menu_width).max(0.0));
        let y = app
            .core
            .context_menu
            .y
            .clamp(0.0, (cx.viewport_h - menu_height).max(0.0));
        let label_budget = (menu_width - padding * 2.0 - tokens::BORDER_THIN * 2.0).max(0.0);
        let rows = app
            .core
            .context_menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| ContextMenuRow {
                label: text_layout::truncate_with_ellipsis(cx, &item.label, label_budget),
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

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiContextMenuHit {
        let root = self.build_tree(cx);
        context_menu_hit_from_id(ui_hit_id(&root, cx, mx, my))
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

    fn build_tree(&self, cx: &UiContext<'_>) -> Div {
        let padding = tokens::SPACE_2;
        let bw = tokens::BORDER_THIN;

        let menu_bg = cx.theme.surface;
        // Subtle sink below term_bg so the panel reads as "recessed chrome"
        // without producing a gamma-incorrect darken — the rest of the
        // pipeline treats colors as sRGB-encoded (see commit 2f609b9), so
        // a raw channel multiply (`[c * 0.9]`) skews hue on non-neutral
        // backgrounds. `surface_sink` applies a flat additive delta that
        // matches the rest of the chrome.
        let bg_color = tokens::surface_sink(
            [menu_bg[0], menu_bg[1], menu_bg[2], 1.0],
            tokens::SURFACE_SINK,
        );
        let border_color = cx.theme.border_focus;
        let accent = cx.theme.accent;
        let fg_color = cx.theme.on_surface;
        let dim_color = cx.theme.on_surface_muted;
        let hover_bg = tokens::tint(accent, tokens::ALPHA_HOVER_BG);

        let content_w = self.menu_width - bw * 2.0;
        let item_h = self.item_height;
        let text_pad = (padding - bw).max(0.0);
        let mut panel = div()
            .in_layer(Layer::Modal)
            .absolute()
            .left(self.x)
            .top(self.y)
            .w(self.menu_width)
            .h(self.menu_height)
            .flex_col()
            .items_center()
            .bg(bg_color)
            .rounded(tokens::SPACE_1)
            .border(bw, border_color)
            .shadow_md()
            .hit_id(HIT_MENU)
            .child(div().w(content_w).h(padding));

        for (index, row) in self.rows.iter().enumerate() {
            let text_color = if row.enabled { fg_color } else { dim_color };
            let mut row_el = div()
                .w(content_w)
                .h(item_h)
                .flex_row()
                .items_center()
                .child(div().w(text_pad).h(item_h))
                .child(text(row.label.clone()).color(text_color));
            if row.enabled {
                row_el = row_el.hit_id(entry_hit_id(index)).cursor_pointer();
            }
            if row.hovered {
                row_el = row_el.bg(hover_bg);
            }
            panel = panel.child(row_el);
        }
        panel = panel.child(div().w(content_w).h(padding));

        let root = div().w(cx.viewport_w).h(cx.viewport_h).child(panel);

        root
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_element_tree(&root, cx, scene);
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
    fn capture_clamps_menu_inside_small_viewport() {
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
            hovered_index: None,
        };
        let mut config = CiriConfig::default();
        config.theme = app.core.config.theme.clone();
        let theme = ciri_ui::ResolvedTheme::default();
        let cx = test_ui_context(&config, &theme, 120.0, 60.0);
        let menu = ContextMenuComponent::capture(&app, &cx).expect("menu visible");
        assert!(menu.x >= 0.0);
        assert!(menu.y >= 0.0);
        assert!(menu.x + menu.menu_width <= cx.viewport_w + 0.001);
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
            hovered_index: None,
        };
        let cx = app.ui_context();
        let menu = ContextMenuComponent::capture(&app, &cx).expect("menu visible");

        assert_eq!(menu.hit_test(60.0, 65.0, &cx), UiContextMenuHit::Entry(0));
        assert_eq!(menu.hit_test(60.0, 90.0, &cx), UiContextMenuHit::Menu);
        assert_eq!(menu.hit_test(10.0, 10.0, &cx), UiContextMenuHit::None);
    }
}
