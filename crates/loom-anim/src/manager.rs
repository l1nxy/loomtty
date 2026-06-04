use std::collections::HashMap;

use crate::anim_value::AnimValue;
use crate::easing::EasingCurve;
use crate::spring::SpringParams;

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
    FadeSlideUp,
}

/// Pane close animation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStyle {
    Fade,
    SlideLeft,
    SlideDown,
    Shrink,
}

/// A single animation context: either spring or easing.
#[derive(Debug, Clone)]
pub enum AnimKind {
    Spring(SpringParams),
    Easing {
        duration_secs: f64,
        curve: EasingCurve,
    },
}

impl AnimKind {
    /// Create an AnimValue from this kind, animating from → to.
    fn create(&self, from: f64, to: f64) -> AnimValue {
        match self {
            AnimKind::Spring(params) => AnimValue::spring(from, to, *params),
            AnimKind::Easing {
                duration_secs,
                curve,
            } => AnimValue::eased(from, to, *duration_secs, *curve),
        }
    }

    /// Start an animation on an existing AnimValue (preserves velocity for springs).
    fn apply(&self, anim: &mut AnimValue, target: f64) {
        match self {
            AnimKind::Spring(params) => anim.animate_to(target, *params),
            AnimKind::Easing {
                duration_secs,
                curve,
            } => anim.ease_to(target, *duration_secs, *curve),
        }
    }
}

/// Pane open animation configuration.
#[derive(Debug, Clone)]
pub struct PaneOpenConfig {
    pub style: OpenStyle,
    pub kind: AnimKind,
}

impl Default for PaneOpenConfig {
    fn default() -> Self {
        Self {
            style: OpenStyle::Fade,
            kind: AnimKind::Easing {
                duration_secs: 0.2,
                curve: EasingCurve::EaseOutCubic,
            },
        }
    }
}

/// Pane close animation configuration.
#[derive(Debug, Clone)]
pub struct PaneCloseConfig {
    pub style: CloseStyle,
    pub kind: AnimKind,
}

impl Default for PaneCloseConfig {
    fn default() -> Self {
        Self {
            style: CloseStyle::Fade,
            kind: AnimKind::Easing {
                duration_secs: 0.15,
                curve: EasingCurve::EaseOutCubic,
            },
        }
    }
}

/// Per-scene animation configuration.
#[derive(Debug, Clone)]
pub struct AnimConfig {
    pub enabled: bool,
    /// View scrolling (horizontal/vertical offset).
    pub view_scroll: AnimKind,
    /// Focus opacity transition between panes.
    pub focus_transition: AnimKind,
    /// Pane open animation.
    pub pane_open: PaneOpenConfig,
    /// Pane close animation.
    pub pane_close: PaneCloseConfig,
    /// Column width resize.
    pub column_resize: AnimKind,
    /// Overview zoom in/out.
    pub overview_zoom: AnimKind,
    /// Bell flash duration in seconds.
    pub bell_flash_secs: f64,
    /// Leader pulse duration in seconds.
    pub leader_pulse_secs: f64,
    /// Opacity for unfocused panes (0.0–1.0).
    pub inactive_opacity: f32,
    /// Pane move animation (when moving pane left/right).
    pub pane_move: AnimKind,
    /// Opacity during resize drag (0.0–1.0).
    pub drag_opacity: f32,
    /// Drag dim animation kind.
    pub drag_dim: AnimKind,
}

impl Default for AnimConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            view_scroll: AnimKind::Spring(SpringParams::default()),
            focus_transition: AnimKind::Spring(SpringParams::snappy()),
            pane_open: PaneOpenConfig::default(),
            pane_close: PaneCloseConfig::default(),
            column_resize: AnimKind::Spring(SpringParams::smooth()),
            overview_zoom: AnimKind::Spring(SpringParams::smooth()),
            bell_flash_secs: 0.15,
            leader_pulse_secs: 0.3,
            inactive_opacity: 0.7,
            pane_move: AnimKind::Spring(SpringParams::default()),
            drag_opacity: 0.6,
            drag_dim: AnimKind::Spring(SpringParams::snappy()),
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
    /// Move offset X (old_pos - new_pos → 0). `None` when idle.
    move_offset_x: Option<AnimValue>,
    /// Move offset Y (old_pos - new_pos → 0). `None` when idle.
    move_offset_y: Option<AnimValue>,
    /// Drag dim opacity (1.0 → drag_opacity during resize drag). `None` when idle.
    drag_dim: Option<AnimValue>,
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
    /// Workspace index that `col_widths` was last synced for.
    /// When the active workspace changes, `sync_col_animations` jumps
    /// instead of animating so stale values don't cause a spurious resize.
    pub col_widths_ws_idx: usize,
    /// Request a one-shot "equalize then settle" animation on the next
    /// `sync_col_animations`. Set on authoritative session switches so the
    /// new layout springs from a uniform average width rather than from
    /// the previous session's unrelated column widths.
    pub col_widths_equalize_pending: bool,

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
            col_widths_ws_idx: 0,
            col_widths_equalize_pending: false,
            pane_anims: HashMap::new(),
            closing: Vec::new(),
            prev_focused: None,
            effects: Vec::new(),
        }
    }

    // ── Lifecycle events ──

    /// Called when a new pane is created. Starts open animation.
    pub fn on_pane_created(&mut self, pane_id: PaneId, config: &AnimConfig) {
        let open_opacity = Some(config.pane_open.kind.create(0.0, 1.0));

        let has_slide = !matches!(config.pane_open.style, OpenStyle::Fade);
        let open_slide = if has_slide {
            Some(config.pane_open.kind.create(1.0, 0.0))
        } else {
            None
        };

        let focus_opacity = AnimValue::new(1.0);

        self.pane_anims.insert(
            pane_id,
            PaneAnimState {
                open_opacity,
                open_slide,
                focus_opacity,
                move_offset_x: None,
                move_offset_y: None,
                drag_dim: None,
            },
        );
    }

    /// Ensure a pane is registered without starting open animations.
    /// Used for panes that already exist on reconnect (StateSync).
    pub fn ensure_pane_registered(&mut self, pane_id: PaneId) {
        self.pane_anims
            .entry(pane_id)
            .or_insert_with(|| PaneAnimState {
                open_opacity: None,
                open_slide: None,
                focus_opacity: AnimValue::new(1.0),
                move_offset_x: None,
                move_offset_y: None,
                drag_dim: None,
            });
    }

    /// Called when a pane is closed. Starts close animation and removes pane state.
    pub fn on_pane_closed(&mut self, pane_id: PaneId, rect: GeoRect, config: &AnimConfig) {
        self.pane_anims.remove(&pane_id);

        if self.prev_focused == Some(pane_id) {
            self.prev_focused = None;
        }

        let slide = match config.pane_close.style {
            CloseStyle::Fade => None,
            _ => Some(config.pane_close.kind.create(0.0, 1.0)),
        };

        self.closing.push(ClosingState {
            rect,
            opacity: config.pane_close.kind.create(1.0, 0.0),
            slide,
        });
    }

    /// Called when focus changes between panes.
    pub fn on_focus_changed(&mut self, new_focus: Option<PaneId>, config: &AnimConfig) {
        let old_focus = self.prev_focused;
        self.prev_focused = new_focus;

        if old_focus == new_focus {
            return;
        }

        // Dim the previously focused pane
        if let Some(old_id) = old_focus
            && let Some(state) = self.pane_anims.get_mut(&old_id)
        {
            config
                .focus_transition
                .apply(&mut state.focus_opacity, config.inactive_opacity as f64);
        }

        // Brighten the newly focused pane
        if let Some(new_id) = new_focus
            && let Some(state) = self.pane_anims.get_mut(&new_id)
        {
            config.focus_transition.apply(&mut state.focus_opacity, 1.0);
        }
    }

    /// Called when a bell fires on a pane.
    pub fn on_bell(&mut self, pane_id: PaneId, config: &AnimConfig) {
        self.effects
            .retain(|e| !matches!(e.kind, EffectKind::BellFlash { pane_id: id } if id == pane_id));

        self.effects.push(TransientEffect {
            kind: EffectKind::BellFlash { pane_id },
            intensity: AnimValue::eased(1.0, 0.0, config.bell_flash_secs, EasingCurve::Linear),
        });
    }

    /// Called when leader mode is activated (pulse effect).
    pub fn on_leader_pulse(&mut self, config: &AnimConfig) {
        self.effects.retain(|e| e.kind != EffectKind::LeaderPulse);
        self.effects.push(TransientEffect {
            kind: EffectKind::LeaderPulse,
            intensity: AnimValue::eased(
                1.0,
                0.0,
                config.leader_pulse_secs,
                EasingCurve::EaseOutCubic,
            ),
        });
    }

    // ── Move animation ──

    /// Start a move animation: pane slides from offset (dx, dy) back to 0.
    /// Used when layout changes cause pane positions to shift.
    pub fn start_move_animation(&mut self, pane_id: PaneId, dx: f32, dy: f32, config: &AnimConfig) {
        if let Some(state) = self.pane_anims.get_mut(&pane_id) {
            if dx.abs() > 1.0 {
                state.move_offset_x = Some(config.pane_move.create(dx as f64, 0.0));
            }
            if dy.abs() > 1.0 {
                state.move_offset_y = Some(config.pane_move.create(dy as f64, 0.0));
            }
        }
    }

    // ── Drag dim ──

    /// Start drag dim: pane opacity fades to drag_opacity.
    pub fn start_drag_dim(&mut self, pane_id: PaneId, config: &AnimConfig) {
        if let Some(state) = self.pane_anims.get_mut(&pane_id) {
            let current = state.drag_dim.as_ref().map_or(1.0, AnimValue::value);
            state.drag_dim = Some(config.drag_dim.create(current, config.drag_opacity as f64));
        }
    }

    /// End drag dim: pane opacity restores to 1.0.
    pub fn end_drag_dim(&mut self, pane_id: PaneId, config: &AnimConfig) {
        if let Some(state) = self.pane_anims.get_mut(&pane_id)
            && let Some(ref mut dim) = state.drag_dim
        {
            // Animation still in progress — re-target to 1.0
            config.drag_dim.apply(dim, 1.0);
        }
        // If drag_dim is None, animation already completed at target (1.0) — no-op
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
    pub fn pane_focus_opacity(&self, pane_id: PaneId) -> f32 {
        self.pane_anims
            .get(&pane_id)
            .map(|s| s.focus_opacity.value() as f32)
            .unwrap_or(1.0)
    }

    /// Get pane move offset (pixels from layout position). (0, 0) when idle.
    pub fn pane_move_offset(&self, pane_id: PaneId) -> (f32, f32) {
        self.pane_anims
            .get(&pane_id)
            .map(|s| {
                let dx = s
                    .move_offset_x
                    .as_ref()
                    .map(|v| v.value() as f32)
                    .unwrap_or(0.0);
                let dy = s
                    .move_offset_y
                    .as_ref()
                    .map(|v| v.value() as f32)
                    .unwrap_or(0.0);
                (dx, dy)
            })
            .unwrap_or((0.0, 0.0))
    }

    /// Get pane drag dim factor (1.0 = normal, lower = dimmed). 1.0 when idle.
    pub fn pane_drag_dim(&self, pane_id: PaneId) -> f32 {
        self.pane_anims
            .get(&pane_id)
            .and_then(|s| s.drag_dim.as_ref())
            .map(|v| v.value() as f32)
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
            if let Some(ref mut v) = state.move_offset_x {
                if !v.advance(dt) {
                    state.move_offset_x = None;
                } else {
                    any = true;
                }
            }
            if let Some(ref mut v) = state.move_offset_y {
                if !v.advance(dt) {
                    state.move_offset_y = None;
                } else {
                    any = true;
                }
            }
            if let Some(ref mut v) = state.drag_dim {
                if !v.advance(dt) {
                    state.drag_dim = None;
                } else {
                    any = true;
                }
            }
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

        // GC completed closing animations (keep if either opacity or slide still running)
        self.closing.retain(|c| {
            c.opacity.is_animating() || c.slide.as_ref().is_some_and(|s| s.is_animating())
        });

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
            || self.pane_anims.values().any(|s| {
                s.focus_opacity.is_animating()
                    || s.move_offset_x.is_some()
                    || s.move_offset_y.is_some()
                    || s.drag_dim.is_some()
            })
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

    fn default_config() -> AnimConfig {
        AnimConfig::default()
    }

    #[test]
    fn pane_open_fade_completes() {
        let mut mgr = AnimationManager::new();
        let config = default_config();
        mgr.on_pane_created(1, &config);

        assert!(mgr.pane_open_opacity(1) < 0.01);
        assert!(mgr.is_animating());

        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert!((mgr.pane_open_opacity(1) - 1.0).abs() < 0.01);
    }

    #[test]
    fn pane_close_cleans_up() {
        let mut mgr = AnimationManager::new();
        let config = default_config();
        mgr.on_pane_created(1, &config);

        for _ in 0..30 {
            mgr.advance_all(1.0 / 60.0);
        }

        let rect = GeoRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        mgr.on_pane_closed(1, rect, &config);

        assert_eq!(mgr.pane_open_opacity(1), 1.0);
        assert!(mgr.closing_panes().next().is_some());

        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert!(mgr.closing_panes().next().is_none());
    }

    #[test]
    fn focus_opacity_no_leak() {
        let mut mgr = AnimationManager::new();
        let config = default_config();

        mgr.on_pane_created(1, &config);
        mgr.on_pane_created(2, &config);
        mgr.on_focus_changed(Some(1), &config);

        let rect = GeoRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        mgr.on_pane_closed(1, rect, &config);

        for _ in 0..30 {
            mgr.advance_all(1.0 / 60.0);
        }

        // Pane 1 has no state — returns default 1.0
        assert_eq!(mgr.pane_focus_opacity(1), 1.0);
    }

    #[test]
    fn focus_change_animates() {
        let mut mgr = AnimationManager::new();
        let config = default_config();

        mgr.on_pane_created(1, &config);
        mgr.on_pane_created(2, &config);
        mgr.on_focus_changed(Some(1), &config);

        for _ in 0..60 {
            mgr.advance_all(1.0 / 60.0);
        }

        mgr.on_focus_changed(Some(2), &config);

        mgr.advance_all(0.05);
        let p1 = mgr.pane_focus_opacity(1);
        let p2 = mgr.pane_focus_opacity(2);
        assert!(p1 < 1.0, "pane 1 should be dimming: {p1}");
        assert!(p2 > 0.7, "pane 2 should be brightening: {p2}");
    }

    #[test]
    fn bell_flash_expires() {
        let mut mgr = AnimationManager::new();
        let config = default_config();

        mgr.on_bell(1, &config);
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
        let config = default_config();
        assert!(!mgr.is_animating());

        mgr.view_offset_x.animate_to(100.0, SpringParams::snappy());
        assert!(mgr.is_animating());

        for _ in 0..300 {
            mgr.advance_all(1.0 / 60.0);
        }
        assert!(!mgr.is_animating());

        mgr.on_leader_pulse(&config);
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
        let mut config = default_config();
        config.pane_open.style = OpenStyle::SlideUp;

        mgr.on_pane_created(1, &config);
        assert!(mgr.pane_open_slide(1) > 0.9);

        for _ in 0..20 {
            mgr.advance_all(1.0 / 60.0);
        }
        assert!(mgr.pane_open_slide(1) < 0.01);
    }

    #[test]
    fn drag_dim_retargets_from_current_value_when_restarted() {
        let mut mgr = AnimationManager::new();
        let config = default_config();

        mgr.ensure_pane_registered(1);
        mgr.start_drag_dim(1, &config);
        mgr.advance_all(0.05);
        let mid_drag_dim = mgr.pane_drag_dim(1);
        assert!(mid_drag_dim < 1.0 && mid_drag_dim > config.drag_opacity);

        mgr.start_drag_dim(1, &config);

        assert!(
            (mgr.pane_drag_dim(1) - mid_drag_dim).abs() < 0.001,
            "restarting drag dim should preserve current dim level"
        );
    }

    #[test]
    fn drag_dim_can_restore_after_restart() {
        let mut mgr = AnimationManager::new();
        let config = default_config();

        mgr.ensure_pane_registered(1);
        mgr.start_drag_dim(1, &config);
        mgr.advance_all(0.05);
        mgr.start_drag_dim(1, &config);
        mgr.end_drag_dim(1, &config);

        for _ in 0..120 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert!((mgr.pane_drag_dim(1) - 1.0).abs() < 0.01);
        assert!(!mgr.is_animating());
    }

    #[test]
    fn spring_based_pane_open() {
        let mut mgr = AnimationManager::new();
        let mut config = default_config();
        config.pane_open.kind = AnimKind::Spring(SpringParams::bouncy());

        mgr.on_pane_created(1, &config);
        assert!(mgr.pane_open_opacity(1) < 0.01);

        for _ in 0..300 {
            mgr.advance_all(1.0 / 60.0);
        }

        assert!((mgr.pane_open_opacity(1) - 1.0).abs() < 0.01);
    }
}
