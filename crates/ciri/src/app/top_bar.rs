use ciri_config::theme::ThemeConfig;

use super::App;

const PANE_TAB_WIDTH_CHARS: usize = 20;

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
    pub workspace_x: f32,
    pub workspace_w: f32,
    pub mode_x: f32,
    pub mode_w: f32,
    pub tabs_area_px: f32,
}

impl App {
    pub(crate) fn hit_test_top_bar(&self, mx: f32, my: f32) -> bool {
        let (cell_w, cell_h) = self.cell_dimensions();
        let layout = self.top_bar_layout(
            self.core.workspaces.view_size.width,
            self.core.workspaces.view_size.height + self.total_chrome_height(),
            cell_w,
            cell_h,
        );
        mx >= 0.0 && my >= layout.bar_y && my <= layout.bar_y + layout.bar_height
    }

    pub(crate) fn ensure_active_pane_tab_visible(&mut self, tab_area_px: f32) {
        if tab_area_px <= 0.0 {
            self.core.pane_tab_scroll = 0.0;
            return;
        }

        let tabs = self.pane_tab_layouts_raw();
        if tabs.is_empty() {
            self.core.pane_tab_scroll = 0.0;
            return;
        }

        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * self.cell_dimensions().0;
        let total_w = tabs.len() as f32 * tab_w;
        let max_scroll = (total_w - tab_area_px).max(0.0);

        let active_idx = tabs.iter().position(|t| t.active).unwrap_or(0);
        let active_start = active_idx as f32 * tab_w;
        let active_end = active_start + tab_w;
        let mut scroll = self.core.pane_tab_scroll.clamp(0.0, max_scroll);
        if active_start < scroll {
            scroll = active_start;
        } else if active_end > scroll + tab_area_px {
            scroll = (active_end - tab_area_px).max(0.0);
        }
        self.core.pane_tab_scroll = scroll.clamp(0.0, max_scroll);
    }

    pub(crate) fn pane_tab_scroll_max(&self) -> f32 {
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
            )
            .tabs_area_px;

        let tab_count = self.pane_tab_entries().len() as f32;
        let total_w = tab_count * PANE_TAB_WIDTH_CHARS as f32 * cw;
        (total_w - tab_area_px).max(0.0)
    }

    pub(crate) fn pane_tab_layouts(&self, cw: f32, tabs_area_px: f32) -> Vec<PaneTabLayout> {
        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * cw;
        let session_w = format!(" {}  ", self.core.session_name).chars().count() as f32 * cw;
        let tabs_start_x = session_w;
        let tabs_end_x = tabs_start_x + tabs_area_px;
        let mut x = tabs_start_x - self.core.pane_tab_scroll;
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

    fn pane_tab_layouts_raw(&self) -> Vec<PaneTabLayout> {
        let tab_w = PANE_TAB_WIDTH_CHARS as f32 * self.cell_dimensions().0;
        let mut layouts = Vec::new();
        for (idx, (pane_id, title)) in self.pane_tab_entries().into_iter().enumerate() {
            let label = self.format_pane_tab_label(idx, &title);
            layouts.push(PaneTabLayout {
                pane_id,
                label,
                x: format!(" {}  ", self.core.session_name).len() as f32 * self.cell_dimensions().0
                    + idx as f32 * tab_w,
                w: tab_w,
                active: self.core.workspaces.active().active_pane_id() == Some(pane_id),
            });
        }
        layouts
    }

    fn pane_tab_entries(&self) -> Vec<(u64, String)> {
        let mut panes = Vec::new();
        let ws = self.core.workspaces.active();
        for col in &ws.columns {
            for tile in &col.tiles {
                let title = self
                    .core.pane_grids
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

    pub(crate) fn workspace_indicator_label(&self) -> String {
        let idx = self.core.workspaces.active_workspace_idx + 1;
        let total = self.core.workspaces.workspaces.len();
        if total <= 1 {
            String::new()
        } else {
            format!("[{}/{}] ", idx, total)
        }
    }

    pub(crate) fn current_mode_label(&self) -> (String, [f32; 4]) {
        let accent = ThemeConfig::parse_color(&self.core.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&self.core.config.theme.mode_broadcast);
        let dim = ThemeConfig::parse_color(&self.core.config.theme.statusbar_dim);
        let warn_color = ThemeConfig::parse_color(&self.core.config.theme.mode_broadcast);
        if self.core.input.is_locked() {
            (" LOCKED ".into(), warn_color)
        } else if self.core.broadcast_mode {
            (" BROADCAST ".into(), broadcast_color)
        } else if self.core.overview.active {
            (" OVERVIEW ".into(), accent)
        } else if let Some(name) = self.core.input.current_mode_name() {
            (format!(" {} ", name.to_uppercase()), accent)
        } else if self.core.input.is_awaiting_action() {
            (" LEADER ".into(), accent)
        } else {
            (" NORMAL ".into(), dim)
        }
    }

    pub(crate) fn top_bar_layout(&self, vw: f32, vh: f32, cw: f32, ch: f32) -> TopBarLayout {
        let padding = if let Some(px) = self.core.config.statusbar.height_padding {
            px
        } else {
            ch * self.core.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        let bar_y = self.status_bar_y(vh);
        let session_w = format!(" {}  ", self.core.session_name).chars().count() as f32 * cw;
        let ws_label = self.workspace_indicator_label();
        let workspace_w = ws_label.chars().count() as f32 * cw;
        let mode_w = self.current_mode_label().0.chars().count() as f32 * cw;
        // Right side: workspace + gap + mode
        let mode_x = vw - mode_w;
        let workspace_x = mode_x - workspace_w;
        let tabs_area_px = (workspace_x - cw - session_w).max(0.0);

        TopBarLayout {
            bar_y,
            bar_height,
            session_x: 0.0,
            session_w,
            workspace_x,
            workspace_w,
            mode_x,
            mode_w,
            tabs_area_px,
        }
    }
}
