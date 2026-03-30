use ciri_anim::anim_value::AnimValue;
use ciri_anim::easing::EasingCurve;
use ciri_anim::manager::{
    AnimConfig, AnimKind, CloseStyle, OpenStyle, PaneCloseConfig, PaneOpenConfig,
};
use ciri_anim::spring::SpringParams;
use ciri_config::config::{AnimationPreset, PaneOpenStyle};
use ciri_protocol::message::BounceDirection;

use super::AppModel;

impl AppModel {
    pub fn anim_config(&self) -> AnimConfig {
        // All timings derived from a single preset
        // stiffness = (2π / response)^2, damping_ratio = 0.86
        let (primary, fast, slow, open_dur, close_dur) = match self.config.animation.preset {
            AnimationPreset::Snappy => (
                SpringParams::new(0.86, 440.0, 0.0001), // ~0.3s
                SpringParams::new(0.86, 800.0, 0.0001), // ~0.2s
                SpringParams::new(0.86, 250.0, 0.0001), // ~0.4s
                0.15,
                0.12,
            ),
            AnimationPreset::Default => (
                SpringParams::new(0.86, 158.0, 0.0001), // ~0.5s
                SpringParams::new(0.86, 440.0, 0.0001), // ~0.3s
                SpringParams::new(0.86, 80.0, 0.0001),  // ~0.7s
                0.25,
                0.18,
            ),
            AnimationPreset::Smooth => (
                SpringParams::new(0.86, 80.0, 0.0001),  // ~0.7s
                SpringParams::new(0.86, 158.0, 0.0001), // ~0.5s
                SpringParams::new(0.86, 40.0, 0.001),   // ~1.0s
                0.35,
                0.25,
            ),
            AnimationPreset::Gentle => (
                SpringParams::new(0.86, 40.0, 0.001),  // ~1.0s
                SpringParams::new(0.86, 80.0, 0.0001), // ~0.7s
                SpringParams::new(0.86, 25.0, 0.001),  // ~1.3s
                0.50,
                0.35,
            ),
        };

        AnimConfig {
            enabled: self.config.animation.enabled,
            view_scroll: AnimKind::Spring(primary),
            focus_transition: AnimKind::Spring(fast),
            pane_open: PaneOpenConfig {
                style: match self.config.animation.pane_open_style {
                    PaneOpenStyle::Fade => OpenStyle::Fade,
                    PaneOpenStyle::SlideUp => OpenStyle::SlideUp,
                    PaneOpenStyle::SlideDown => OpenStyle::SlideDown,
                    PaneOpenStyle::SlideLeft => OpenStyle::SlideLeft,
                    PaneOpenStyle::FadeSlideUp => OpenStyle::FadeSlideUp,
                },
                kind: AnimKind::Easing {
                    duration_secs: open_dur,
                    curve: EasingCurve::EaseOutCubic,
                },
            },
            pane_close: PaneCloseConfig {
                style: CloseStyle::Fade,
                kind: AnimKind::Easing {
                    duration_secs: close_dur,
                    curve: EasingCurve::EaseOutCubic,
                },
            },
            column_resize: AnimKind::Spring(slow),
            overview_zoom: AnimKind::Spring(slow),
            bell_flash_secs: 0.15,
            leader_pulse_secs: 0.3,
            inactive_opacity: self.config.appearance.inactive_opacity,
            pane_move: AnimKind::Spring(primary),
            drag_opacity: self.config.animation.drag_opacity,
            drag_dim: AnimKind::Spring(fast),
        }
    }

    /// Helper: spring params for scroll/zoom from config.
    pub fn scroll_spring(&self) -> SpringParams {
        SpringParams::default()
    }

    pub fn snap_all_col_widths(&mut self) {
        let vw = self.workspaces.view_size.width;
        for ws in &mut self.workspaces.workspaces {
            for col in &mut ws.columns {
                col.snap_width(vw);
            }
        }
    }

    pub fn refresh_overview_zoom(&mut self) {
        if !self.overview.active {
            return;
        }
        let sp = self.scroll_spring();
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let max_w = self
            .workspaces
            .workspaces
            .iter()
            .map(|ws| ws.total_width())
            .fold(0.0f32, f32::max)
            .max(vw);
        let nrows = self
            .workspaces
            .workspaces
            .iter()
            .filter(|ws| !ws.is_empty())
            .count()
            .max(1);
        let total_h =
            nrows as f32 * vh + (nrows.saturating_sub(1)) as f32 * self.workspaces.workspace_gap;
        let fit = self.config.animation.overview_zoom_fit;
        let zoom_x = vw / max_w;
        let zoom_y = vh / total_h;
        let zoom = (zoom_x.min(zoom_y).min(1.0) * fit).max(0.15);
        self.anim_mgr.overview_zoom.animate_to(zoom as f64, sp);
    }

    pub fn animate_to_active(&mut self) {
        let sp = self.scroll_spring();
        let enabled = self.config.animation.enabled;

        let center_strategy = match self.config.layout.center_focused_column {
            ciri_config::config::CenterStrategy::Always => {
                ciri_layout::workspace::CenterStrategy::Always
            }
            ciri_config::config::CenterStrategy::OnOverflow => {
                ciri_layout::workspace::CenterStrategy::OnOverflow
            }
            ciri_config::config::CenterStrategy::Never => {
                ciri_layout::workspace::CenterStrategy::Never
            }
        };
        // Use the animation target (not in-flight value) so offset calculation
        // is based on where we *intend* to be, not where we are mid-spring.
        let intended_vox = self.anim_mgr.view_offset_x.target() as f32;
        let target_x = self
            .workspaces
            .active_mut()
            .target_offset_for_active_with_strategy(center_strategy, intended_vox);
        if !enabled {
            self.anim_mgr.view_offset_x.jump_to(target_x as f64);
        } else if (self.anim_mgr.view_offset_x.target() - target_x as f64).abs() > 0.5 {
            self.anim_mgr.view_offset_x.animate_to(target_x as f64, sp);
        }

        let target_y = self.workspaces.target_offset_y();
        if !enabled {
            self.anim_mgr.view_offset_y.jump_to(target_y as f64);
        } else if (self.anim_mgr.view_offset_y.target() - target_y as f64).abs() > 0.5 {
            self.anim_mgr.view_offset_y.animate_to(target_y as f64, sp);
        }

        self.sync_col_animations();
    }

    /// Rubber-band bounce when focus hits an edge boundary.
    /// Nudges the view offset slightly in the bounce direction, then springs back.
    pub fn bounce_edge(&mut self, direction: BounceDirection) {
        if !self.config.animation.enabled {
            return;
        }
        let cw = self.workspaces.view_size.width;
        let ch = self.workspaces.view_size.height;
        // Nudge distance: ~3% of viewport dimension
        let nudge = match direction {
            BounceDirection::Left | BounceDirection::Right => cw * 0.03,
            BounceDirection::Up | BounceDirection::Down => ch * 0.03,
        };
        let sp = SpringParams::new(0.7, 600.0, 0.0001);

        match direction {
            BounceDirection::Left => {
                let target = self.anim_mgr.view_offset_x.target();
                self.anim_mgr.view_offset_x.jump_to(target - nudge as f64);
                self.anim_mgr.view_offset_x.animate_to(target, sp);
            }
            BounceDirection::Right => {
                let target = self.anim_mgr.view_offset_x.target();
                self.anim_mgr.view_offset_x.jump_to(target + nudge as f64);
                self.anim_mgr.view_offset_x.animate_to(target, sp);
            }
            BounceDirection::Up => {
                let target = self.anim_mgr.view_offset_y.target();
                self.anim_mgr.view_offset_y.jump_to(target - nudge as f64);
                self.anim_mgr.view_offset_y.animate_to(target, sp);
            }
            BounceDirection::Down => {
                let target = self.anim_mgr.view_offset_y.target();
                self.anim_mgr.view_offset_y.jump_to(target + nudge as f64);
                self.anim_mgr.view_offset_y.animate_to(target, sp);
            }
        }
    }

    pub fn sync_col_animations(&mut self) {
        let ncols = self.workspaces.active().columns.len();
        while self.anim_mgr.col_widths.len() < ncols {
            self.anim_mgr.col_widths.push(AnimValue::new(0.0));
        }
        self.anim_mgr.col_widths.truncate(ncols);

        let vw = self.workspaces.active().view_size.width;
        let sp = self.scroll_spring();
        for (i, col) in self.workspaces.active().columns.iter().enumerate() {
            let target = col.resolve_width(vw) as f64;
            let current = self.anim_mgr.col_widths[i].value();
            let anim_target = self.anim_mgr.col_widths[i].target();
            if self.config.animation.enabled {
                if current == 0.0 {
                    // New column: animate from average neighbor width for smooth entry
                    let neighbor = if i > 0 {
                        self.anim_mgr.col_widths[i - 1].target()
                    } else if i + 1 < ncols {
                        // Next column hasn't been set yet, use target
                        self.workspaces
                            .active()
                            .columns
                            .get(i + 1)
                            .map(|c| c.resolve_width(vw) as f64)
                            .unwrap_or(target)
                    } else {
                        target
                    };
                    self.anim_mgr.col_widths[i].jump_to(neighbor);
                    self.anim_mgr.col_widths[i].animate_to(target, sp);
                } else if (anim_target - target).abs() > 1.0 {
                    self.anim_mgr.col_widths[i].animate_to(target, sp);
                }
            } else {
                self.anim_mgr.col_widths[i].jump_to(target);
            }
        }

        let ws = self.workspaces.active_mut();
        for (i, col) in ws.columns.iter_mut().enumerate() {
            if i < self.anim_mgr.col_widths.len() {
                col.set_rendered_width(self.anim_mgr.col_widths[i].value() as f32);
            }
        }
    }

    pub fn advance_animations(&mut self, dt: f64) -> bool {
        // sync_col_animations keeps col_widths targets up-to-date with layout
        if !self.anim_mgr.col_widths.is_empty() {
            self.sync_col_animations();
        }
        // advance_all ticks view_offset_x/y, overview_zoom, gesture_row_offset,
        // col_widths, and per-pane/effect animations.
        let animating = self.anim_mgr.advance_all(dt);
        // Apply advanced col_width values back to rendered layout
        let ws = self.workspaces.active_mut();
        for (i, col) in ws.columns.iter_mut().enumerate() {
            if i < self.anim_mgr.col_widths.len() {
                col.set_rendered_width(self.anim_mgr.col_widths[i].value() as f32);
            }
        }
        animating
    }
}
