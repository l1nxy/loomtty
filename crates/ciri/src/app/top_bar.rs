use ciri_config::theme::ThemeConfig;

use super::status_bar::{build_hints_from_bindings, find_key_for_action};
use super::App;

const PANE_TAB_WIDTH_CHARS: usize = 14;

#[derive(Debug, Clone)]
pub(crate) struct PaneTabLayout {
    pub pane_id: u64,
    pub label: String,
    pub x: f32,
    pub w: f32,
    pub active: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TopBarLayout {
    pub bar_y: f32,
    pub bar_height: f32,
    pub session_x: f32,
    pub session_w: f32,
    pub hints_x: f32,
    pub hints_w: f32,
    pub mode_x: f32,
    pub mode_w: f32,
    pub tabs_area_px: f32,
}

impl App {
    pub(crate) fn hit_test_top_bar(&self, mx: f32, my: f32) -> bool {
        let Some(atlas) = self.glyph_cache.as_ref() else {
            return false;
        };
        let layout = self.top_bar_layout(
            self.workspaces.view_size.width,
            self.workspaces.view_size.height + self.status_bar_height(),
            atlas.cell_width,
            atlas.cell_height,
        );
        mx >= 0.0 && my >= layout.bar_y && my <= layout.bar_y + layout.bar_height
    }

    pub(crate) fn ensure_active_pane_tab_visible(&mut self, tab_area_px: f32) {
        if tab_area_px <= 0.0 {
            self.pane_tab_scroll = 0.0;
            return;
        }

        let tabs = self.pane_tab_layouts_raw();
        if tabs.is_empty() {
            self.pane_tab_scroll = 0.0;
            return;
        }

        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * self.cell_dimensions().0;
        let total_w = tabs.len() as f32 * tab_w;
        let max_scroll = (total_w - tab_area_px).max(0.0);

        let active_idx = tabs.iter().position(|t| t.active).unwrap_or(0);
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
        let ch = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.config.font.size * 1.2);
        let cw = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_width)
            .unwrap_or(8.0);
        let tab_area_px = self
            .top_bar_layout(
                self.workspaces.view_size.width,
                self.workspaces.view_size.height + self.status_bar_height(),
                cw,
                ch,
            )
            .tabs_area_px;

        let tab_count = self.pane_tab_entries().len() as f32;
        let total_w = tab_count * PANE_TAB_WIDTH_CHARS as f32 * cw;
        (total_w - tab_area_px).max(0.0)
    }

    pub(crate) fn pane_tab_layouts(&self, cw: f32, tabs_area_px: f32) -> Vec<PaneTabLayout> {
        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * cw;
        let tabs_start_x = format!(" {}  ", self.session_name).len() as f32 * cw;
        let tabs_end_x = tabs_start_x + tabs_area_px;
        let mut x = tabs_start_x - self.pane_tab_scroll;
        let mut layouts = Vec::new();

        for (idx, (pane_id, title)) in self.pane_tab_entries().into_iter().enumerate() {
            let label = self.format_pane_tab_label(idx, &title);
            let active = self.workspaces.active().active_pane_id() == Some(pane_id);
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

    fn pane_tab_layouts_raw(&self) -> Vec<PaneTabLayout> {
        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * self.cell_dimensions().0;
        let mut layouts = Vec::new();
        for (idx, (pane_id, title)) in self.pane_tab_entries().into_iter().enumerate() {
            let label = self.format_pane_tab_label(idx, &title);
            layouts.push(PaneTabLayout {
                pane_id,
                label,
                x: format!(" {}  ", self.session_name).len() as f32 * self.cell_dimensions().0
                    + idx as f32 * tab_w,
                w: tab_w,
                active: self.workspaces.active().active_pane_id() == Some(pane_id),
            });
        }
        layouts
    }

    fn pane_tab_entries(&self) -> Vec<(u64, String)> {
        let mut panes = Vec::new();
        let ws = self.workspaces.active();
        for col in &ws.columns {
            for tile in &col.tiles {
                let title = self
                    .pane_grids
                    .get(&tile.pane_id)
                    .map(|g| g.title.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "pane".to_string());
                panes.push((tile.pane_id, title));
            }
        }
        panes
    }

    fn format_pane_tab_label(&self, idx: usize, title: &str) -> String {
        let prefix = format!("{:>2} ", idx + 1);
        let max_title_chars = PANE_TAB_WIDTH_CHARS.saturating_sub(prefix.len());
        let mut title = title.chars().take(max_title_chars).collect::<String>();
        let label = format!("{}{}", prefix, title);
        title.clear();
        format!("{:<width$}", label, width = PANE_TAB_WIDTH_CHARS)
    }

    pub(crate) fn current_mode_label(&self) -> (&'static str, [f32; 4]) {
        let accent = ThemeConfig::parse_color(&self.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&self.config.theme.mode_broadcast);
        let dim = ThemeConfig::parse_color(&self.config.theme.statusbar_dim);
        if self.broadcast_mode {
            (" BROADCAST ", broadcast_color)
        } else if self.overview.active {
            (" OVERVIEW ", accent)
        } else if self.input.is_awaiting_action() {
            (" LEADER ", accent)
        } else {
            (" NORMAL ", dim)
        }
    }

    pub(crate) fn statusbar_hints(&self) -> String {
        if self.broadcast_mode {
            let key = find_key_for_action(&self.config.keys.bindings, "toggle_broadcast");
            format!(
                "{}:exit broadcast  leader:{}",
                key,
                self.config.keys.leader.to_uppercase()
            )
        } else if self.overview.active {
            build_hints_from_bindings(
                &self.config.keys.overview_bindings,
                &[
                    ("focus_left", "\u{2190}"),
                    ("focus_right", "\u{2192}"),
                    ("focus_up", "\u{2191}"),
                    ("focus_down", "\u{2193}"),
                    ("close_pane", "close"),
                    ("new_column_right", "new"),
                    ("exit_overview", "exit"),
                ],
            )
        } else if self.input.is_awaiting_action() {
            let is_sticky = self.config.input.mode == "sticky";
            let bindings = &self.config.keys.bindings;
            let core = build_hints_from_bindings(
                bindings,
                &[
                    ("new_column_right", "new"),
                    ("close_pane", "close"),
                    ("focus_left", "\u{2190}"),
                    ("focus_right", "\u{2192}"),
                    ("focus_up", "\u{2191}"),
                    ("focus_down", "\u{2193}"),
                    ("cycle_preset_width", "width"),
                    ("toggle_overview", "overview"),
                    ("detach", "detach"),
                ],
            );
            let extended = build_hints_from_bindings(
                bindings,
                &[
                    ("new_row_below", "split"),
                    ("move_pane_left", "mv\u{2190}"),
                    ("move_pane_right", "mv\u{2192}"),
                    ("column_width_decrease", "w-"),
                    ("column_width_increase", "w+"),
                    ("column_width_full", "full"),
                    ("consume_into_column", "stack"),
                    ("expel_from_column", "unstack"),
                    ("toggle_broadcast", "broadcast"),
                ],
            );
            let mut h = if extended.is_empty() {
                core.clone()
            } else {
                format!("{}  {}", core, extended)
            };
            if is_sticky {
                h.push_str("  esc:exit");
            }
            h
        } else {
            format!("leader:{}", self.config.keys.leader.to_uppercase())
        }
    }

    pub(crate) fn top_bar_layout(&self, vw: f32, vh: f32, cw: f32, ch: f32) -> TopBarLayout {
        let padding = if let Some(px) = self.config.statusbar.height_padding {
            px
        } else {
            ch * self.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        let bar_y = self.status_bar_y(vh);
        let session_w = format!(" {}  ", self.session_name).len() as f32 * cw;
        let hints_w = self.statusbar_hints().len() as f32 * cw;
        let mode_w = self.current_mode_label().0.len() as f32 * cw;
        let mode_x = (vw - mode_w).max(0.0);
        let hints_x = (mode_x - 2.0 * cw - hints_w).max(session_w);
        let tabs_area_px = (hints_x - session_w).max(0.0);

        TopBarLayout {
            bar_y,
            bar_height,
            session_x: 0.0,
            session_w,
            hints_x,
            hints_w,
            mode_x,
            mode_w,
            tabs_area_px,
        }
    }
}
