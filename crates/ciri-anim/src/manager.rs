use std::collections::HashMap;

use crate::anim_value::{AnimValue, EasingCurve};

pub type PaneId = u64;

/// Geometric rectangle for closing pane snapshots.
#[derive(Debug, Clone, Copy)]
pub struct GeoRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Pane open animation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStyle {
    Fade,
    SlideUp,
    SlideDown,
    SlideLeft,
    SlideRight,
    FadeSlideUp,
    FadeSlideDown,
}

/// Pane close animation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStyle {
    Fade,
    SlideLeft,
    SlideRight,
    SlideDown,
    Shrink,
}

/// Parameters for creating animations, read from config at call site.
#[derive(Debug, Clone)]
pub struct AnimParams {
    pub omega: f64,
    pub epsilon: f64,
    pub focus_speed: f64,
    pub open_style: OpenStyle,
    pub open_duration_secs: f64,
    pub close_style: CloseStyle,
    pub close_duration_secs: f64,
    pub bell_duration_secs: f64,
    pub inactive_opacity: f32,
}

impl Default for AnimParams {
    fn default() -> Self {
        Self {
            omega: 12.0,
            epsilon: 0.1,
            focus_speed: 15.0,
            open_style: OpenStyle::Fade,
            open_duration_secs: 0.2,
            close_style: CloseStyle::Fade,
            close_duration_secs: 0.15,
            bell_duration_secs: 0.15,
            inactive_opacity: 0.7,
        }
    }
}

/// Per-pane animation state.
#[derive(Debug)]
struct PaneAnimState {
    /// Opacity fade-in (0→1). `None` once complete.
    open_opacity: Option<AnimValue>,
    /// Slide-in offset (1→0). `None` if not sliding or complete.
    open_slide: Option<AnimValue>,
    /// Focus opacity (1.0=active, inactive_opacity=inactive). Always present.
    focus_opacity: AnimValue,
}

/// State for a pane that is closing (fading/sliding out).
#[derive(Debug)]
struct ClosingState {
    rect: GeoRect,
    opacity: AnimValue,
    slide: Option<AnimValue>,
}

/// Short-lived visual effect.
#[derive(Debug)]
struct TransientEffect {
    kind: EffectKind,
    /// Animated value: 1.0→0.0 over the effect duration.
    intensity: AnimValue,
}

/// Types of transient effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    BellFlash { pane_id: PaneId },
    LeaderPulse,
}

/// Centralized animation state for the entire application.
#[derive(Debug)]
pub struct AnimationManager {
    // ── Global animations ──
    pub view_offset_x: AnimValue,
    pub view_offset_y: AnimValue,
    pub overview_zoom: AnimValue,
    pub gesture_row_offset: AnimValue,
    pub col_widths: Vec<AnimValue>,

    // ── Per-pane animations ──
    pane_anims: HashMap<PaneId, PaneAnimState>,
    closing: Vec<ClosingState>,
    prev_focused: Option<PaneId>,

    // ── Transient effects ──
    effects: Vec<TransientEffect>,
}

impl AnimationManager {
    pub fn new() -> Self {
        Self {
            view_offset_x: AnimValue::new(0.0),
            view_offset_y: AnimValue::new(0.0),
            overview_zoom: AnimValue::new(1.0),
            gesture_row_offset: AnimValue::new(0.0),
            col_widths: Vec::new(),
            pane_anims: HashMap::new(),
            closing: Vec::new(),
            prev_focused: None,
            effects: Vec::new(),
        }
    }

    // ── Lifecycle events ──

    /// Called when a new pane is created. Starts open animation.
    pub fn on_pane_created(&mut self, pane_id: PaneId, params: &AnimParams) {
        let dur = params.open_duration_secs;
        let curve = EasingCurve::EaseOutCubic;

        let open_opacity = Some(AnimValue::eased(0.0, 1.0, dur, curve));

        let has_slide = !matches!(params.open_style, OpenStyle::Fade);
        let open_slide = if has_slide {
            Some(AnimValue::eased(1.0, 0.0, dur, curve))
        } else {
            None
        };

        let focus_opacity = AnimValue::new(1.0); // starts focused

        self.pane_anims.insert(
            pane_id,
            PaneAnimState {
                open_opacity,
                open_slide,
                focus_opacity,
            },
        );
    }

    /// Ensure a pane is registered in the animation system without starting open animations.
    /// Used for panes that already exist on reconnect (StateSync).
    pub fn ensure_pane_registered(&mut self, pane_id: PaneId) {
        self.pane_anims.entry(pane_id).or_insert_with(|| PaneAnimState {
            open_opacity: None,
            open_slide: None,
            focus_opacity: AnimValue::new(1.0),
        });
    }

    /// Called when a pane is closed. Starts close animation and removes pane state.
    pub fn on_pane_closed(&mut self, pane_id: PaneId, rect: GeoRect, params: &AnimParams) {
        // Remove live pane state (including focus_opacity — no leak)
        self.pane_anims.remove(&pane_id);

        if self.prev_focused == Some(pane_id) {
            self.prev_focused = None;
        }

        let dur = params.close_duration_secs;
        let curve = EasingCurve::EaseOutCubic;

        let slide = match params.close_style {
            CloseStyle::Fade => None,
            _ => Some(AnimValue::eased(0.0, 1.0, dur, curve)),
        };

        self.closing.push(ClosingState {
            rect,
            opacity: AnimValue::eased(1.0, 0.0, dur, curve),
            slide,
        });
    }

    /// Called when focus changes between panes.
    pub fn on_focus_changed(
        &mut self,
        new_focus: Option<PaneId>,
        params: &AnimParams,
    ) {
        let old_focus = self.prev_focused;
        self.prev_focused = new_focus;

        if old_focus == new_focus {
            return;
        }

        // Dim the previously focused pane
        if let Some(old_id) = old_focus {
            if let Some(state) = self.pane_anims.get_mut(&old_id) {
                state.focus_opacity.animate_to(
                    params.inactive_opacity as f64,
                    params.focus_speed,
                    params.epsilon,
                );
            }
        }

        // Brighten the newly focused pane
        if let Some(new_id) = new_focus {
            if let Some(state) = self.pane_anims.get_mut(&new_id) {
                state.focus_opacity.animate_to(1.0, params.focus_speed, params.epsilon);
            }
        }
    }

    /// Called when a bell fires on a pane.
    pub fn on_bell(&mut self, pane_id: PaneId, params: &AnimParams) {
        // Remove any existing bell for this pane
        self.effects
            .retain(|e| !matches!(e.kind, EffectKind::BellFlash { pane_id: id } if id == pane_id));

        self.effects.push(TransientEffect {
            kind: EffectKind::BellFlash { pane_id },
            intensity: AnimValue::eased(1.0, 0.0, params.bell_duration_secs, EasingCurve::Linear),
        });
    }

    /// Called when leader mode is activated (pulse effect).
    pub fn on_leader_pulse(&mut self, duration_secs: f64) {
        self.effects.retain(|e| e.kind != EffectKind::LeaderPulse);
        self.effects.push(TransientEffect {
            kind: EffectKind::LeaderPulse,
            intensity: AnimValue::eased(1.0, 0.0, duration_secs, EasingCurve::EaseOutCubic),
        });
    }

    // ── Query (for rendering) ──

    /// Get pane open opacity (0→1 during open animation, 1.0 when complete).
    pub fn pane_open_opacity(&self, pane_id: PaneId) -> f32 {
        self.pane_anims
            .get(&pane_id)
            .and_then(|s| s.open_opacity.as_ref())
            .map(|v| v.value() as f32)
            .unwrap_or(1.0)
    }

    /// Get pane open slide progress (1→0 during animation, 0.0 when complete).
    pub fn pane_open_slide(&self, pane_id: PaneId) -> f32 {
        self.pane_anims
            .get(&pane_id)
            .and_then(|s| s.open_slide.as_ref())
            .map(|v| v.value() as f32)
            .unwrap_or(0.0)
    }

    /// Get pane focus opacity (1.0 for active, inactive_opacity for inactive).
    pub fn pane_focus_opacity(&self, pane_id: PaneId, _inactive_opacity: f32) -> f32 {
        self.pane_anims
            .get(&pane_id)
            .map(|s| s.focus_opacity.value() as f32)
            .unwrap_or(1.0)
    }

    /// Iterate closing panes: (rect, opacity, slide_offset).
    pub fn closing_panes(&self) -> impl Iterator<Item = (GeoRect, f32, f32)> + '_ {
        self.closing.iter().map(|c| {
            let slide = c.slide.as_ref().map(|s| s.value() as f32).unwrap_or(0.0);
            (c.rect, c.opacity.value() as f32, slide)
        })
    }

    /// Get bell flash intensity for a pane (0.0 if no active bell).
    pub fn bell_flash(&self, pane_id: PaneId) -> f32 {
        self.effects
            .iter()
            .find(|e| matches!(e.kind, EffectKind::BellFlash { pane_id: id } if id == pane_id))
            .map(|e| e.intensity.value() as f32)
            .unwrap_or(0.0)
    }

    /// Get leader pulse intensity (0.0 if no active pulse).
    pub fn leader_pulse(&self) -> f32 {
        self.effects
            .iter()
            .find(|e| e.kind == EffectKind::LeaderPulse)
            .map(|e| e.intensity.value() as f32)
            .unwrap_or(0.0)
    }

    /// Whether any pane has an active open animation.
    pub fn has_open_anims(&self) -> bool {
        self.pane_anims
            .values()
            .any(|s| s.open_opacity.is_some() || s.open_slide.is_some())
    }

    // ── Tick ──

    /// Advance all animations by `dt` seconds. Returns true if any still running.
    pub fn advance_all(&mut self, dt: f64) -> bool {
        let mut any = false;

        any |= self.view_offset_x.advance(dt);
        any |= self.view_offset_y.advance(dt);
        any |= self.overview_zoom.advance(dt);
        any |= self.gesture_row_offset.advance(dt);

        for cw in &mut self.col_widths {
            any |= cw.advance(dt);
        }

        for state in self.pane_anims.values_mut() {
            if let Some(ref mut v) = state.open_opacity {
                if !v.advance(dt) {
                    state.open_opacity = None;
                } else {
                    any = true;
                }
            }
            if let Some(ref mut v) = state.open_slide {
                if !v.advance(dt) {
                    state.open_slide = None;
                } else {
                    any = true;
                }
            }
            any |= state.focus_opacity.advance(dt);
        }

        for c in &mut self.closing {
            any |= c.opacity.advance(dt);
            if let Some(ref mut s) = c.slide {
                any |= s.advance(dt);
            }
        }

        self.effects.retain_mut(|e| {
            let running = e.intensity.advance(dt);
            any |= running;
            running
        });

        // GC completed closing animations
        self.closing.retain(|c| c.opacity.is_animating());

        any
    }

    /// Whether any animation is currently running.
    pub fn is_animating(&self) -> bool {
        self.view_offset_x.is_animating()
            || self.view_offset_y.is_animating()
            || self.overview_zoom.is_animating()
            || self.gesture_row_offset.is_animating()
            || self.col_widths.iter().any(|c| c.is_animating())
            || self.has_open_anims()
            || self.pane_anims.values().any(|s| s.focus_opacity.is_animating())
            || !self.closing.is_empty()
            || !self.effects.is_empty()
    }
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> AnimParams {
        AnimParams::default()
    }

    #[test]
    fn pane_open_fade_completes() {
        let mut mgr = AnimationManager::new();
        let params = default_params();
        mgr.on_pane_created(1, &params);

        assert!(mgr.pane_open_opacity(1) < 0.01);
        assert!(mgr.is_animating());

        // Run 200ms at 60fps
        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert!((mgr.pane_open_opacity(1) - 1.0).abs() < 0.01);
    }

    #[test]
    fn pane_close_cleans_up() {
        let mut mgr = AnimationManager::new();
        let params = default_params();
        mgr.on_pane_created(1, &params);

        // Complete open animation
        for _ in 0..30 {
            mgr.advance_all(1.0 / 60.0);
        }

        let rect = GeoRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        mgr.on_pane_closed(1, rect, &params);

        // Pane state removed, closing state added
        assert_eq!(mgr.pane_open_opacity(1), 1.0); // default (no entry)
        assert!(mgr.closing_panes().next().is_some());

        // Run 150ms closing
        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }

        // Closing state GC'd
        assert!(mgr.closing_panes().next().is_none());
    }

    #[test]
    fn focus_opacity_no_leak() {
        let mut mgr = AnimationManager::new();
        let params = default_params();

        mgr.on_pane_created(1, &params);
        mgr.on_pane_created(2, &params);
        mgr.on_focus_changed(Some(1), &params);

        // Close pane 1 — focus_opacity entry must be removed
        let rect = GeoRect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        mgr.on_pane_closed(1, rect, &params);

        // After closing animation completes
        for _ in 0..30 {
            mgr.advance_all(1.0 / 60.0);
        }

        // Pane 1 should have no state at all
        assert_eq!(mgr.pane_focus_opacity(1, 0.7), 0.7); // falls to default
    }

    #[test]
    fn focus_change_animates() {
        let mut mgr = AnimationManager::new();
        let params = default_params();

        mgr.on_pane_created(1, &params);
        mgr.on_pane_created(2, &params);
        mgr.on_focus_changed(Some(1), &params);

        // Complete open + initial focus
        for _ in 0..60 {
            mgr.advance_all(1.0 / 60.0);
        }

        // Switch focus to pane 2
        mgr.on_focus_changed(Some(2), &params);

        // Pane 1 should start dimming, pane 2 should start brightening
        mgr.advance_all(0.05);
        let p1 = mgr.pane_focus_opacity(1, 0.7);
        let p2 = mgr.pane_focus_opacity(2, 0.7);
        assert!(p1 < 1.0, "pane 1 should be dimming: {p1}");
        assert!(p2 > 0.7, "pane 2 should be brightening: {p2}");
    }

    #[test]
    fn bell_flash_expires() {
        let mut mgr = AnimationManager::new();
        let params = default_params();

        mgr.on_bell(1, &params);
        assert!(mgr.bell_flash(1) > 0.5);
        assert!(mgr.is_animating());

        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert_eq!(mgr.bell_flash(1), 0.0);
        assert!(mgr.effects.is_empty());
    }

    #[test]
    fn is_animating_reflects_all_sources() {
        let mut mgr = AnimationManager::new();
        assert!(!mgr.is_animating());

        // Global animation
        mgr.view_offset_x.animate_to(100.0, 12.0, 0.1);
        assert!(mgr.is_animating());

        for _ in 0..300 {
            mgr.advance_all(1.0 / 60.0);
        }
        assert!(!mgr.is_animating());

        // Effect
        mgr.on_leader_pulse(0.1);
        assert!(mgr.is_animating());
    }

    #[test]
    fn advance_all_returns_false_when_idle() {
        let mut mgr = AnimationManager::new();
        assert!(!mgr.advance_all(1.0 / 60.0));
    }

    #[test]
    fn slide_open_style() {
        let mut mgr = AnimationManager::new();
        let mut params = default_params();
        params.open_style = OpenStyle::SlideUp;

        mgr.on_pane_created(1, &params);
        assert!(mgr.pane_open_slide(1) > 0.9); // starts at 1.0

        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }
        assert!(mgr.pane_open_slide(1) < 0.01); // ends at 0.0
    }
}
