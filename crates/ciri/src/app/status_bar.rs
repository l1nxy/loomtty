use ciri_config::theme::ThemeConfig;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::rect::Rect;

use super::App;

const PANE_TAB_WIDTH_CHARS: usize = 14;

#[derive(Debug, Clone)]
struct PaneTabLayout {
    pane_id: u64,
    label: String,
    x: f32,
    w: f32,
    active: bool,
}

#[derive(Debug, Clone, Copy)]
struct TopBarLayout {
    bar_height: f32,
    session_x: f32,
    session_w: f32,
    hints_x: f32,
    hints_w: f32,
    mode_x: f32,
    mode_w: f32,
    tabs_area_px: f32,
}

impl App {
    pub fn build_status_bar(
        &mut self,
        vw: f32,
        _vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let ch = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.config.font.size * 1.2);
        let padding = if let Some(px) = self.config.statusbar.height_padding {
            px
        } else {
            ch * self.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        let bar_y = 0.0;
        let cw = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_width)
            .unwrap_or(8.0);
        let baseline = ch * self.config.statusbar.text_baseline;
        let text_y = bar_y + padding * 0.5;

        let bar_bg = ThemeConfig::parse_color(&self.config.theme.statusbar_background);
        let dim = ThemeConfig::parse_color(&self.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&self.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&self.config.theme.mode_broadcast);

        bg_rects.push(Rect {
            x: 0.0,
            y: bar_y,
            w: vw,
            h: bar_height,
            color: bar_bg,
        });

        let is_leader = self.input.is_awaiting_action();
        let is_broadcast = self.broadcast_mode;
        let is_overview = self.overview.active;

        let (mode_label, mode_color) = if is_broadcast {
            (" BROADCAST ", broadcast_color)
        } else if is_overview {
            (" OVERVIEW ", accent)
        } else if is_leader {
            (" LEADER ", accent)
        } else {
            (" NORMAL ", dim)
        };

        let leader_key = self.config.keys.leader.to_uppercase();
        let hints = if is_broadcast {
            let key = find_key_for_action(&self.config.keys.bindings, "toggle_broadcast");
            format!("{}:exit broadcast  leader:{}", key, leader_key)
        } else if is_overview {
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
        } else if is_leader {
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

            let avail = (vw / cw) as usize;
            let mode_chars = mode_label.len() + 2;
            let left_chars = self.session_name.len() + 2;
            let budget = avail.saturating_sub(mode_chars + left_chars);

            let full = if extended.is_empty() {
                core.clone()
            } else {
                format!("{}  {}", core, extended)
            };

            let mut h = if full.len() <= budget { full } else { core };
            if is_sticky {
                h.push_str("  esc:exit");
            }
            h
        } else {
            format!("leader:{}", leader_key)
        };

        let session_text = format!(" {}  ", self.session_name);
        let layout = self.top_bar_layout(vw, cw, ch);
        let right_segments: Vec<(&str, [f32; 4])> =
            vec![(&hints, dim), ("  ", dim), (mode_label, mode_color)];
        let right_chars: usize = right_segments.iter().map(|(s, _)| s.len()).sum();
        let available = (vw / cw) as usize;
        let tab_area_px = layout.tabs_area_px;
        self.ensure_active_pane_tab_visible(tab_area_px);

        let tabs_start_x = layout.session_x + layout.session_w;
        let tabs_end_x = tabs_start_x + tab_area_px;
        let pane_tabs = self.pane_tab_layouts(cw, tab_area_px);
        let atlas = self.glyph_cache.as_mut().unwrap();

        emit_status_text(
            atlas,
            &session_text,
            0.0,
            text_y,
            cw,
            baseline,
            dim,
            glyphs,
        );

        for tab in &pane_tabs {
            let mut tab_bg = if tab.active { accent } else { dim };
            tab_bg[3] = if tab.active { 0.18 } else { 0.10 };
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }
            bg_rects.push(Rect {
                x: visible_left - 2.0,
                y: bar_y + 1.0,
                w: visible_w + 4.0,
                h: bar_height - 2.0,
                color: tab_bg,
            });
            if let Some((label, label_x)) =
                clip_tab_label(&tab.label, tab.x, tab.w, cw, tabs_start_x, tabs_end_x)
            {
                emit_status_text(
                    atlas,
                    &label,
                    label_x,
                    text_y,
                    cw,
                    baseline,
                    if tab.active { accent } else { dim },
                    glyphs,
                );
            }
        }

        // Render right segments (right-aligned, always shown)
        let right_text_chars: usize = right_chars.min(available);
        let mut rx = vw - right_text_chars as f32 * cw;
        for (text, color) in &right_segments {
            emit_status_text(atlas, text, rx, text_y, cw, baseline, *color, glyphs);
            rx += text.len() as f32 * cw;
        }

        // Mode indicator line below the top bar (same visual cue, but positioned
        // above the content area rather than at the bottom of the window).
        if is_leader || is_broadcast || is_overview {
            let indicator_h = ch * self.config.statusbar.leader_indicator_ratio;
            let indicator_color = if is_broadcast { broadcast_color } else { accent };
            bg_rects.push(Rect {
                x: 0.0,
                y: bar_height,
                w: vw,
                h: indicator_h,
                color: indicator_color,
            });
        }

        // Mode label background pill.
        let pill_x = vw - mode_label.len() as f32 * cw;
        let mut pill_bg = mode_color;
        pill_bg[3] = 0.15;
        bg_rects.push(Rect {
            x: pill_x,
            y: bar_y,
            w: mode_label.len() as f32 * cw,
            h: bar_height,
            color: pill_bg,
        });
    }

    pub(crate) fn hit_test_pane_tab(&self, mx: f32, my: f32) -> Option<u64> {
        let atlas = self.glyph_cache.as_ref()?;
        let cw = atlas.cell_width;
        let ch = atlas.cell_height;
        let layout = self.top_bar_layout(self.workspaces.view_size.width, cw, ch);
        if my < 0.0 || my > layout.bar_height {
            return None;
        }
        let tabs_area_px = layout.tabs_area_px;

        for tab in self.pane_tab_layouts(cw, tabs_area_px) {
            if mx >= tab.x && mx <= tab.x + tab.w {
                return Some(tab.pane_id);
            }
        }
        None
    }

    pub(crate) fn hit_test_session_name(&self, mx: f32, my: f32) -> bool {
        let Some(atlas) = self.glyph_cache.as_ref() else {
            return false;
        };
        let layout = self.top_bar_layout(
            self.workspaces.view_size.width,
            atlas.cell_width,
            atlas.cell_height,
        );
        mx >= layout.session_x
            && mx <= layout.session_x + layout.session_w
            && my >= 0.0
            && my <= layout.bar_height
    }

    pub(crate) fn hit_test_mode_pill(&self, mx: f32, my: f32) -> bool {
        let Some(atlas) = self.glyph_cache.as_ref() else {
            return false;
        };
        let layout = self.top_bar_layout(
            self.workspaces.view_size.width,
            atlas.cell_width,
            atlas.cell_height,
        );
        mx >= layout.mode_x
            && mx <= layout.mode_x + layout.mode_w
            && my >= 0.0
            && my <= layout.bar_height
    }

    pub(crate) fn hit_test_leader_hint(&self, mx: f32, my: f32) -> bool {
        let Some(atlas) = self.glyph_cache.as_ref() else {
            return false;
        };
        if self.broadcast_mode || self.overview.active || self.input.is_awaiting_action() {
            return false;
        }
        let layout = self.top_bar_layout(
            self.workspaces.view_size.width,
            atlas.cell_width,
            atlas.cell_height,
        );
        layout.hints_w > 0.0
            && mx >= layout.hints_x
            && mx <= layout.hints_x + layout.hints_w
            && my >= 0.0
            && my <= layout.bar_height
    }

    pub(crate) fn hit_test_top_bar(&self, mx: f32, my: f32) -> bool {
        let Some(atlas) = self.glyph_cache.as_ref() else {
            return false;
        };
        let ch = atlas.cell_height;
        let padding = if let Some(px) = self.config.statusbar.height_padding {
            px
        } else {
            ch * self.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        mx >= 0.0 && my >= 0.0 && my <= bar_height
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
            .top_bar_layout(self.workspaces.view_size.width, cw, ch)
            .tabs_area_px;

        let tab_count = self.pane_tab_entries().len() as f32;
        let total_w = tab_count * PANE_TAB_WIDTH_CHARS as f32 * cw;
        (total_w - tab_area_px).max(0.0)
    }

    fn pane_tab_layouts(&self, cw: f32, tabs_area_px: f32) -> Vec<PaneTabLayout> {
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

    fn current_mode_label(&self) -> (&'static str, [f32; 4]) {
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

    fn statusbar_hints(&self) -> String {
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

    fn top_bar_layout(&self, vw: f32, cw: f32, ch: f32) -> TopBarLayout {
        let padding = if let Some(px) = self.config.statusbar.height_padding {
            px
        } else {
            ch * self.config.statusbar.padding_ratio
        };
        let bar_height = ch + padding;
        let session_w = format!(" {}  ", self.session_name).len() as f32 * cw;
        let hints_w = self.statusbar_hints().len() as f32 * cw;
        let mode_w = self.current_mode_label().0.len() as f32 * cw;
        let mode_x = (vw - mode_w).max(0.0);
        let hints_x = (mode_x - 2.0 * cw - hints_w).max(session_w);
        let tabs_area_px = (hints_x - session_w).max(0.0);

        TopBarLayout {
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

fn clip_tab_label(
    label: &str,
    tab_x: f32,
    tab_w: f32,
    cw: f32,
    tabs_start_x: f32,
    tabs_end_x: f32,
) -> Option<(String, f32)> {
    let visible_left = tab_x.max(tabs_start_x);
    let visible_right = (tab_x + tab_w).min(tabs_end_x);
    if visible_right <= visible_left {
        return None;
    }
    let skip_left = ((visible_left - tab_x) / cw).floor().max(0.0) as usize;
    let visible_chars = ((visible_right - visible_left) / cw).floor().max(0.0) as usize;
    let clipped = label.chars().skip(skip_left).take(visible_chars).collect::<String>();
    if clipped.is_empty() {
        None
    } else {
        Some((clipped, visible_left))
    }
}

/// Find the key bound to a given action in a bindings map. Returns "?" if not found.
pub(crate) fn find_key_for_action(
    bindings: &std::collections::HashMap<String, String>,
    action: &str,
) -> String {
    for (key, act) in bindings {
        if act == action {
            return key.clone();
        }
    }
    "?".to_string()
}

/// Build hints string from a bindings map and an ordered list of (action, label) pairs.
/// Groups adjacent keys that share the same label (e.g. h/l for left/right become "h/l:leftright").
/// Only includes actions that have a key binding.
pub(crate) fn build_hints_from_bindings(
    bindings: &std::collections::HashMap<String, String>,
    action_labels: &[(&str, &str)],
) -> String {
    let mut action_to_keys: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (key, action) in bindings {
        action_to_keys
            .entry(action.as_str())
            .or_default()
            .push(key.as_str());
    }
    for keys in action_to_keys.values_mut() {
        keys.sort_by_key(|k| k.len());
    }

    let mut parts = Vec::new();
    for (action, label) in action_labels {
        if let Some(keys) = action_to_keys.get(action) {
            let key_str = if keys.len() == 1 {
                keys[0].to_string()
            } else {
                keys.iter().take(2).copied().collect::<Vec<_>>().join("/")
            };
            parts.push(format!("{}:{}", key_str, label));
        }
    }
    parts.join("  ")
}

pub(crate) fn emit_status_text(
    atlas: &mut GlyphCache,
    text: &str,
    x_start: f32,
    text_y: f32,
    cell_width: f32,
    baseline: f32,
    color: [f32; 4],
    glyphs: &mut Vec<GlyphInstance>,
) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(entry) = atlas.ensure_char(ch)
            && entry.width > 0
            && entry.height > 0
        {
            let sx = x_start + i as f32 * cell_width + entry.bearing_x as f32;
            let sy = text_y + baseline - entry.bearing_y as f32;
            glyphs.push(GlyphInstance {
                pos: [sx, sy],
                size: [entry.width as f32, entry.height as f32],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color,
            });
        }
    }
}
