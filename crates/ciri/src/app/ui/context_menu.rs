//! Right-click context menu.
//!
//! A small modal popup anchored at the click point. Rows show the menu item
//! label; enabled rows highlight on hover. Capture clamps the menu inside the
//! viewport so painting can stay purely absolute.


use super::builder::UiBuilder;
use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiComponent, UiContext, UiContextMenuHit, UiScene};
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

    pub(super) fn hit_test(&self, mx: f32, my: f32) -> UiContextMenuHit {
        if mx < self.x
            || mx > self.x + self.menu_width
            || my < self.y
            || my > self.y + self.menu_height
        {
            return UiContextMenuHit::None;
        }
        let padding = tokens::SPACE_2;
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
}

impl UiComponent for ContextMenuComponent {
    fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiContextMenuHit::Entry(idx) => Some(UiAction::ExecuteContextMenuEntry(idx)),
            UiContextMenuHit::Menu => None,
            UiContextMenuHit::None => Some(UiAction::CloseContextMenu),
        }
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
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

        // Outer panel: SDF rounded rect + border + drop shadow, the first
        // overlay in the client to actually drive the new SDF pipeline.
        // Everything else (hover strips, labels) still uses the legacy
        // `UiBuilder` path so this stays a focused migration.
        scene.sdf_rects.push(ciri_render::sdf_rect::SdfRect {
            pos: [self.x, self.y],
            size: [self.menu_width, self.menu_height],
            color: bg_color,
            radii: [tokens::SPACE_1; 4],
            border_color,
            border_width: bw,
            shadow_blur: tokens::SPACE_2,
            shadow_offset: [0.0, tokens::SPACE_1],
            shadow_color: [0.0, 0.0, 0.0, 0.35],
        });

        let mut ui = UiBuilder::new_vertical(
            self.x,
            self.y,
            self.menu_width,
            self.menu_height,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        let content_w = self.menu_width - bw * 2.0;
        let content_h = self.rows.len() as f32 * self.item_height;
        let item_h = self.item_height;
        ui.vertical(content_w, Some(content_h + padding * 2.0), 0.0, |ui| {
            ui.bg_rect(content_w, padding, [0.0; 4]);
            for row in &self.rows {
                ui.horizontal(Some(content_w), item_h, 0.0, |ui| {
                    let (rx, ry) = ui.cursor_pos();
                    if row.hovered {
                        ui.abs_rect(self.x + bw, ry, content_w, item_h, hover_bg);
                    }
                    let text_y = ry + (item_h - cx.ui_line_h) * 0.5;
                    ui.abs_text(
                        &row.label,
                        rx + padding,
                        text_y,
                        if row.enabled { fg_color } else { dim_color },
                    );
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let cx = UiContext {
            config: &config,
            theme: &theme,
            viewport_w: 120.0,
            viewport_h: 60.0,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline: 12.0,
            ui_line_h: 16.0,
            ui_shaper: None,
        };
        let menu = ContextMenuComponent::capture(&app, &cx).expect("menu visible");
        assert!(menu.x >= 0.0);
        assert!(menu.y >= 0.0);
        assert!(menu.x + menu.menu_width <= cx.viewport_w + 0.001);
    }
}
