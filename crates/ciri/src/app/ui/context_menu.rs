use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
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
        let padding = 8.0;
        let bw = 1.0;

        let menu_bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let bg_color = [menu_bg[0] * 0.9, menu_bg[1] * 0.9, menu_bg[2] * 0.9, 1.0];
        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let fg_color = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim_base = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let dim_color = [dim_base[0], dim_base[1], dim_base[2], 0.75];
        let hover_bg = [accent[0], accent[1], accent[2], 0.12];

        let mut ui = UiBuilder::new_vertical(
            self.x, self.y, self.menu_width, self.menu_height, 0.0,
            0.0, 0.0, false, cx, scene,
        );

        // Shadow + border + background
        ui.bordered_panel_inset(
            self.x, self.y, self.menu_width, self.menu_height,
            bg_color, border_color, bw, true,
        );

        // Vertical item list inside the panel (after top padding)
        let content_w = self.menu_width - bw * 2.0;
        let content_h = self.rows.len() as f32 * self.item_height;
        let item_h = self.item_height;
        let cell_h = cx.cell_h;
        ui.vertical(content_w, Some(content_h + padding * 2.0), 0.0, |ui| {
            ui.bg_rect(content_w, padding, [0.0, 0.0, 0.0, 0.0]); // top padding
            for row in &self.rows {
                ui.horizontal(Some(content_w), item_h, 0.0, |ui| {
                    // Hover highlight (full row width)
                    if row.hovered {
                        let (_, ry) = ui.cursor_pos();
                        ui.abs_rect(self.x + bw, ry, content_w, item_h, hover_bg);
                    }
                    // Vertically centered label
                    let (rx, ry) = ui.cursor_pos();
                    let text_y = ry + (item_h - cell_h) * 0.5;
                    ui.abs_text(
                        &row.label, rx + padding, text_y,
                        if row.enabled { fg_color } else { dim_color },
                    );
                });
            }
        });
    }
}
