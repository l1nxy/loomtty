use ciri_config::theme::ThemeConfig;
use ciri_render::rect::Rect;

use super::types::{UiAction, UiComponent, UiContext, UiContextMenuHit, UiScene};
use crate::app::status_bar::{TextEmitParams, emit_status_text};
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
        let shadow_offset = 3.0;
        scene.bg_rects.push(Rect {
            x: self.x + shadow_offset,
            y: self.y + shadow_offset,
            w: self.menu_width,
            h: self.menu_height,
            color: [0.0, 0.0, 0.0, 0.4],
        });

        let menu_bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let bg_color = [menu_bg[0] * 0.9, menu_bg[1] * 0.9, menu_bg[2] * 0.9, 1.0];
        scene.bg_rects.push(Rect {
            x: self.x,
            y: self.y,
            w: self.menu_width,
            h: self.menu_height,
            color: bg_color,
        });

        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let bw = 1.0;
        scene.bg_rects.push(Rect {
            x: self.x,
            y: self.y,
            w: self.menu_width,
            h: bw,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.x,
            y: self.y + self.menu_height - bw,
            w: self.menu_width,
            h: bw,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.x,
            y: self.y,
            w: bw,
            h: self.menu_height,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.x + self.menu_width - bw,
            y: self.y,
            w: bw,
            h: self.menu_height,
            color: border_color,
        });

        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let fg_color = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim_base = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let dim_color = [dim_base[0], dim_base[1], dim_base[2], 0.75];
        for (i, row) in self.rows.iter().enumerate() {
            let iy = self.y + padding + i as f32 * self.item_height;
            if row.hovered {
                scene.bg_rects.push(Rect {
                    x: self.x + bw,
                    y: iy,
                    w: self.menu_width - bw * 2.0,
                    h: self.item_height,
                    color: [accent[0], accent[1], accent[2], 0.12],
                });
            }
            let text_y = iy + (self.item_height - cx.cell_h) * 0.5;
            emit_status_text(
                scene.atlas,
                &row.label,
                &TextEmitParams {
                    x_start: self.x + padding,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: if row.enabled { fg_color } else { dim_color },
                },
                scene.glyphs,
            );
        }
    }
}
