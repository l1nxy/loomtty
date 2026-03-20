use ciri_config::theme::ThemeConfig;
use ciri_render::glyph_cache::{GlyphAtlas, GlyphInstance};
use ciri_render::rect::Rect;

use super::App;

impl App {
    pub fn build_status_bar(
        &mut self,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();

        let bar_height = atlas.cell_height + self.config.statusbar.height_padding;
        let bar_y = vh - bar_height;
        let cw = atlas.cell_width;
        let baseline = atlas.cell_height * self.config.statusbar.text_baseline;
        let text_y = bar_y + 2.0;

        // Colors
        let bar_bg = ThemeConfig::parse_color(&self.config.theme.statusbar_background);
        let dim = ThemeConfig::parse_color(&self.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&self.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&self.config.theme.mode_broadcast);

        // Background
        bg_rects.push(Rect { x: 0.0, y: bar_y, w: vw, h: bar_height, color: bar_bg });

        // Determine mode
        let is_leader = self.input.is_awaiting_action();
        let is_broadcast = self.broadcast_mode;
        let is_overview = self.overview_active;

        let (mode_label, mode_color) = if is_broadcast {
            (" BROADCAST ", broadcast_color)
        } else if is_overview {
            (" OVERVIEW ", accent)
        } else if is_leader {
            (" LEADER ", accent)
        } else {
            (" NORMAL ", dim)
        };

        // === Build left segments: session + workspaces ===
        let session_text = format!(" {}  ", self.session_name);
        let ws_idx = self.workspaces.active_workspace_idx;
        let mut left_segments: Vec<(String, [f32; 4])> = Vec::new();
        left_segments.push((session_text, dim));
        for (i, ws) in self.workspaces.workspaces.iter().enumerate() {
            if !ws.is_empty() || i == ws_idx {
                let label = format!("[{}] ", i + 1);
                let color = if i == ws_idx { accent } else { dim };
                left_segments.push((label, color));
            }
        }

        // Build keybinding hints dynamically from config
        let leader_key = self.config.keys.leader.to_uppercase();
        let hints = if is_broadcast {
            let key = find_key_for_action(&self.config.keys.bindings, "toggle_broadcast");
            format!("{}:exit broadcast  leader:{}", key, leader_key)
        } else if is_overview {
            build_hints_from_bindings(&self.config.keys.overview_bindings, &[
                ("focus_left",  "\u{2190}"), ("focus_right", "\u{2192}"),
                ("focus_up",    "\u{2191}"), ("focus_down",  "\u{2193}"),
                ("close_pane",  "close"), ("new_column_right", "new"),
                ("exit_overview", "exit"),
            ])
        } else if is_leader {
            let is_sticky = self.config.input.mode == "sticky";
            let bindings = &self.config.keys.bindings;

            // Core hints (always shown)
            let core = build_hints_from_bindings(bindings, &[
                ("new_column_right",     "new"),
                ("close_pane",           "close"),
                ("focus_left",           "\u{2190}"),
                ("focus_right",          "\u{2192}"),
                ("focus_up",             "\u{2191}"),
                ("focus_down",           "\u{2193}"),
                ("cycle_preset_width",   "width"),
                ("toggle_overview",      "overview"),
                ("detach",               "detach"),
            ]);

            // Extended hints (shown if space allows)
            let extended = build_hints_from_bindings(bindings, &[
                ("new_row_below",        "split"),
                ("move_pane_left",       "mv\u{2190}"),
                ("move_pane_right",      "mv\u{2192}"),
                ("column_width_decrease","w-"),
                ("column_width_increase","w+"),
                ("column_width_full",    "full"),
                ("consume_into_column",  "stack"),
                ("expel_from_column",    "unstack"),
                ("toggle_broadcast",     "broadcast"),
            ]);

            let avail = (vw / cw) as usize;
            let mode_chars = mode_label.len() + 2; // "  " + mode
            let left_chars: usize = left_segments.iter().map(|(s, _)| s.len()).sum::<usize>();
            let budget = avail.saturating_sub(mode_chars + left_chars);

            let full = if extended.is_empty() {
                core.clone()
            } else {
                format!("{}  {}", core, extended)
            };

            let mut h = if full.len() <= budget {
                full
            } else {
                core
            };
            if is_sticky {
                h.push_str("  esc:exit");
            }
            h
        } else {
            format!("leader:{}", leader_key)
        };

        // === Build right segments: mode + hints ===
        let right_segments: Vec<(&str, [f32; 4])> = vec![
            (&hints, dim),
            ("  ", dim),
            (mode_label, mode_color),
        ];

        // Progressive degradation: hints are priority, drop left segments if needed.
        let right_chars: usize = right_segments.iter().map(|(s, _)| s.len()).sum();
        let available = (vw / cw) as usize;
        let left_budget = available.saturating_sub(right_chars);

        // Render left segments, truncating if they don't fit
        let mut x = 0.0;
        let mut left_used = 0usize;
        for (text, color) in &left_segments {
            if left_used + text.len() > left_budget {
                break; // drop remaining left segments
            }
            emit_status_text(
                atlas, &mut renderer.font_system, &renderer.queue,
                text, x, text_y, cw, baseline, *color, glyphs,
            );
            x += text.len() as f32 * cw;
            left_used += text.len();
        }

        // Render right segments (right-aligned, always shown)
        let right_text_chars: usize = right_chars.min(available);
        let mut rx = vw - right_text_chars as f32 * cw;

        for (text, color) in &right_segments {
            emit_status_text(
                atlas, &mut renderer.font_system, &renderer.queue,
                text, rx, text_y, cw, baseline, *color, glyphs,
            );
            rx += text.len() as f32 * cw;
        }

        // Mode indicator line above status bar (for non-normal modes)
        if is_leader || is_broadcast || is_overview {
            let indicator_h = self.config.statusbar.leader_indicator_height;
            let indicator_color = if is_broadcast { broadcast_color } else { accent };
            bg_rects.push(Rect {
                x: 0.0,
                y: bar_y - indicator_h,
                w: vw,
                h: indicator_h,
                color: indicator_color,
            });
        }

        // Mode label background pill
        let pill_x = {
            vw - mode_label.len() as f32 * cw
        };
        let mut pill_bg = mode_color;
        pill_bg[3] = 0.15; // translucent
        bg_rects.push(Rect {
            x: pill_x,
            y: bar_y,
            w: mode_label.len() as f32 * cw,
            h: bar_height,
            color: pill_bg,
        });
    }
}

/// Find the key bound to a given action in a bindings map. Returns "?" if not found.
pub(crate) fn find_key_for_action(bindings: &std::collections::HashMap<String, String>, action: &str) -> String {
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
    // Reverse map: action -> list of keys (sorted shortest first)
    let mut action_to_keys: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for (key, action) in bindings {
        action_to_keys.entry(action.as_str()).or_default().push(key.as_str());
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
    atlas: &mut GlyphAtlas,
    font_system: &mut glyphon::FontSystem,
    queue: &wgpu::Queue,
    text: &str,
    x_start: f32,
    text_y: f32,
    cell_width: f32,
    baseline: f32,
    color: [f32; 4],
    glyphs: &mut Vec<GlyphInstance>,
) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(entry) = atlas.ensure_char(ch, font_system, queue)
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
