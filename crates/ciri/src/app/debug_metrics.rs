//! Frame-pipeline debug metrics — drives the "Gotta Go Fast"-style overlay.
//!
//! Collects per-phase timings (Build / Layout / Paint / Render), per-event
//! handler timings (Key / Mouse), repaint ratio, and last-frame byte counts.
//! Each metric keeps a small ring buffer so the overlay can show
//! Last / P95 / P99 columns. Disabled by default; enabling forces continuous
//! redraws (the overlay needs fresh numbers every frame to be useful).

use std::time::Instant;

/// Number of samples retained per metric. 256 ≈ ~4s at 60fps which is
/// long enough for P99 to be meaningful but short enough that one stutter
/// fades quickly.
pub(crate) const SAMPLE_CAP: usize = 256;

/// Ring buffer of `f32` samples with O(1) push and lazy percentile compute.
#[derive(Debug, Default)]
pub(crate) struct History {
    samples: Vec<f32>,
    cursor: usize,
    len: usize,
}

impl History {
    pub fn push(&mut self, v: f32) {
        if self.samples.len() < SAMPLE_CAP {
            self.samples.push(v);
        } else {
            self.samples[self.cursor] = v;
            self.cursor = (self.cursor + 1) % SAMPLE_CAP;
        }
        self.len = self.samples.len();
    }

    pub fn last(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        // Most recent insert: when buffer is full we just wrote at
        // `cursor-1`; when growing it's at the back.
        let idx = if self.samples.len() < SAMPLE_CAP {
            self.samples.len() - 1
        } else {
            (self.cursor + SAMPLE_CAP - 1) % SAMPLE_CAP
        };
        Some(self.samples[idx])
    }

    pub fn percentile(&self, p: f32) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted: Vec<f32> = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((p.clamp(0.0, 1.0) * (sorted.len() as f32 - 1.0)).round() as usize)
            .min(sorted.len() - 1);
        Some(sorted[idx])
    }
}

/// Snapshot ready for the overlay UI: Last / P95 / P99 in the metric's
/// natural unit (ms for timings, bytes for the upload row).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Stats {
    pub last: f32,
    pub p95: f32,
    pub p99: f32,
}

impl Stats {
    fn from(h: &History) -> Self {
        Self {
            last: h.last().unwrap_or(0.0),
            p95: h.percentile(0.95).unwrap_or(0.0),
            p99: h.percentile(0.99).unwrap_or(0.0),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DebugMetrics {
    pub enabled: bool,

    key: History,
    mouse: History,
    build: History,
    layout: History,
    paint: History,
    render: History,
    frame: History,
    bytes: History,

    frame_attempts: u64,
    repaints: u64,

    frame_start: Option<Instant>,
    phase_start: Option<(Phase, Instant)>,

    /// Bumps every time a sample is pushed. Used by the UI cache key
    /// so the cached chrome scene rebuilds while the overlay is on,
    /// instead of the cache holding a stale (but still valid) scene.
    pub generation: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Phase {
    Build,
    Layout,
    Paint,
    Render,
}

impl DebugMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled != on {
            self.enabled = on;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn toggle(&mut self) {
        self.set_enabled(!self.enabled);
    }

    /// Frame-attempt counter advances even when the damage check skips
    /// the redraw — that's how we measure repaint ratio.
    pub fn begin_frame_attempt(&mut self) {
        if !self.enabled {
            return;
        }
        self.frame_attempts = self.frame_attempts.saturating_add(1);
        self.frame_start = Some(Instant::now());
    }

    /// Called when the damage check decides nothing changed and `render()`
    /// is about to early-return. The frame-time sample is still pushed
    /// (so the overlay can show very small "Frame" numbers when we're
    /// idle), but no per-phase samples are recorded.
    pub fn end_frame_skipped(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(start) = self.frame_start.take() {
            self.push_ms(MetricKind::Frame, start.elapsed().as_secs_f32() * 1000.0);
        }
    }

    /// `was_forced_by_overlay = true` means the damage check would have
    /// skipped this frame, but the overlay's own continuous-redraw mode
    /// forced it through. Frame-time and byte samples are still recorded
    /// (the work really happened), but the repaint counter only ticks
    /// for repaints the user-facing app actually needed — otherwise the
    /// ratio shows 100% just because the overlay is on.
    pub fn end_frame_repaint(&mut self, bytes: usize, was_forced_by_overlay: bool) {
        if !self.enabled {
            return;
        }
        if !was_forced_by_overlay {
            self.repaints = self.repaints.saturating_add(1);
        }
        if let Some(start) = self.frame_start.take() {
            self.push_ms(MetricKind::Frame, start.elapsed().as_secs_f32() * 1000.0);
        }
        self.bytes.push(bytes as f32);
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn begin_phase(&mut self, phase: Phase) {
        if !self.enabled {
            return;
        }
        self.phase_start = Some((phase, Instant::now()));
    }

    pub fn end_phase(&mut self, phase: Phase) {
        if !self.enabled {
            return;
        }
        if let Some((open, start)) = self.phase_start.take() {
            // Mismatch (nested or reordered scopes) — discard rather than
            // mis-attribute. The overlay will still get samples from
            // future frames where the scopes pair up correctly.
            if !matches_phase(open, phase) {
                return;
            }
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            self.push_ms(
                match phase {
                    Phase::Build => MetricKind::Build,
                    Phase::Layout => MetricKind::Layout,
                    Phase::Paint => MetricKind::Paint,
                    Phase::Render => MetricKind::Render,
                },
                ms,
            );
        }
    }

    pub fn record_key_event(&mut self, ms: f32) {
        if !self.enabled {
            return;
        }
        self.push_ms(MetricKind::Key, ms);
    }

    pub fn record_mouse_event(&mut self, ms: f32) {
        if !self.enabled {
            return;
        }
        self.push_ms(MetricKind::Mouse, ms);
    }

    fn push_ms(&mut self, kind: MetricKind, ms: f32) {
        match kind {
            MetricKind::Key => self.key.push(ms),
            MetricKind::Mouse => self.mouse.push(ms),
            MetricKind::Build => self.build.push(ms),
            MetricKind::Layout => self.layout.push(ms),
            MetricKind::Paint => self.paint.push(ms),
            MetricKind::Render => self.render.push(ms),
            MetricKind::Frame => self.frame.push(ms),
        }
    }

    /// Snapshot all metrics for the overlay UI to render this frame.
    pub fn snapshot(&self) -> DebugSnapshot {
        DebugSnapshot {
            key: Stats::from(&self.key),
            mouse: Stats::from(&self.mouse),
            build: Stats::from(&self.build),
            layout: Stats::from(&self.layout),
            paint: Stats::from(&self.paint),
            render: Stats::from(&self.render),
            frame: Stats::from(&self.frame),
            bytes: Stats::from(&self.bytes),
            repaint_ratio: if self.frame_attempts == 0 {
                0.0
            } else {
                self.repaints as f32 / self.frame_attempts as f32
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MetricKind {
    Key,
    Mouse,
    Build,
    Layout,
    Paint,
    Render,
    Frame,
}

fn matches_phase(open: Phase, end: Phase) -> bool {
    matches!(
        (open, end),
        (Phase::Build, Phase::Build)
            | (Phase::Layout, Phase::Layout)
            | (Phase::Paint, Phase::Paint)
            | (Phase::Render, Phase::Render)
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DebugSnapshot {
    pub key: Stats,
    pub mouse: Stats,
    pub build: Stats,
    pub layout: Stats,
    pub paint: Stats,
    pub render: Stats,
    pub frame: Stats,
    pub bytes: Stats,
    pub repaint_ratio: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_last_and_percentiles() {
        let mut h = History::default();
        for v in 1..=100 {
            h.push(v as f32);
        }
        assert_eq!(h.last(), Some(100.0));
        // Tolerance accounts for the rank-rounding in `percentile` —
        // p50 of 100 samples can land on either side of 50 by one slot.
        assert!((h.percentile(0.0).unwrap() - 1.0).abs() < 0.5);
        assert!((h.percentile(0.5).unwrap() - 50.5).abs() <= 1.0);
        assert!((h.percentile(0.95).unwrap() - 96.0).abs() <= 1.0);
        assert!((h.percentile(0.99).unwrap() - 100.0).abs() <= 1.0);
    }

    #[test]
    fn history_wraps_at_cap() {
        let mut h = History::default();
        for v in 0..(SAMPLE_CAP + 50) {
            h.push(v as f32);
        }
        // Capacity stays at SAMPLE_CAP and last() reflects the most
        // recent push, not the oldest sample.
        assert_eq!(h.samples.len(), SAMPLE_CAP);
        assert_eq!(h.last(), Some((SAMPLE_CAP + 49) as f32));
    }

    #[test]
    fn disabled_metrics_record_nothing() {
        let mut m = DebugMetrics::new();
        m.begin_frame_attempt();
        m.begin_phase(Phase::Build);
        m.end_phase(Phase::Build);
        m.end_frame_repaint(123, false);
        m.record_key_event(1.0);
        let s = m.snapshot();
        assert_eq!(s.frame.last, 0.0);
        assert_eq!(s.build.last, 0.0);
        assert_eq!(s.key.last, 0.0);
        assert_eq!(s.bytes.last, 0.0);
    }

    #[test]
    fn repaint_ratio_tracks_skipped_vs_painted() {
        let mut m = DebugMetrics::new();
        m.set_enabled(true);
        for _ in 0..3 {
            m.begin_frame_attempt();
            m.end_frame_repaint(10, false);
        }
        for _ in 0..7 {
            m.begin_frame_attempt();
            m.end_frame_skipped();
        }
        let s = m.snapshot();
        // 3 painted out of 10 attempts.
        assert!((s.repaint_ratio - 0.3).abs() < 0.0001);
    }

    #[test]
    fn overlay_forced_repaints_do_not_inflate_ratio() {
        let mut m = DebugMetrics::new();
        m.set_enabled(true);
        // 1 real repaint, 9 forced-by-overlay (all paint, none of them
        // were "needed" by the app). Ratio should reflect just the real one.
        m.begin_frame_attempt();
        m.end_frame_repaint(10, false);
        for _ in 0..9 {
            m.begin_frame_attempt();
            m.end_frame_repaint(10, true);
        }
        let s = m.snapshot();
        assert!((s.repaint_ratio - 0.1).abs() < 0.0001);
    }

    #[test]
    fn mismatched_phase_end_does_not_record() {
        let mut m = DebugMetrics::new();
        m.set_enabled(true);
        m.begin_phase(Phase::Build);
        m.end_phase(Phase::Layout); // wrong end — discard
        let s = m.snapshot();
        assert_eq!(s.build.last, 0.0);
        assert_eq!(s.layout.last, 0.0);
    }
}
