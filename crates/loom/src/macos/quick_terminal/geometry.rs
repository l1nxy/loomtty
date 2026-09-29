//! Cocoa screen coordinates are in points, with Y increasing upwards. Keep
//! every animation frame inside visibleFrame so a display above the selected
//! display never sees a sliding window spill into it.
use std::time::{Duration, Instant};

use loom_config::{MacosQuickTerminalConfig, QuickTerminalPosition};
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub fn target_frame(screen: NSRect, config: &MacosQuickTerminalConfig) -> NSRect {
    let width = screen.size.width * config.width.clamp(0.1, 1.0);
    let height = screen.size.height * config.height.clamp(0.1, 1.0);
    let x = match config.position {
        QuickTerminalPosition::Left => screen.origin.x,
        QuickTerminalPosition::Right => screen.origin.x + screen.size.width - width,
        _ => screen.origin.x + (screen.size.width - width) / 2.0,
    };
    let y = match config.position {
        QuickTerminalPosition::Top => screen.origin.y + screen.size.height - height,
        QuickTerminalPosition::Bottom => screen.origin.y,
        _ => screen.origin.y + (screen.size.height - height) / 2.0,
    };
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub fn revealed_frame(target: NSRect, edge: QuickTerminalPosition, progress: f64) -> NSRect {
    let mut frame = target;
    let progress = progress.clamp(0.0, 1.0);
    match edge {
        QuickTerminalPosition::Top | QuickTerminalPosition::Bottom => {
            frame.size.height = (target.size.height * progress).max(1.0);
            if edge == QuickTerminalPosition::Top {
                frame.origin.y += target.size.height - frame.size.height;
            }
        }
        QuickTerminalPosition::Left | QuickTerminalPosition::Right => {
            frame.size.width = (target.size.width * progress).max(1.0);
            if edge == QuickTerminalPosition::Right {
                frame.origin.x += target.size.width - frame.size.width;
            }
        }
    }
    frame
}

pub struct Slide {
    start: Instant,
    duration: Duration,
    from: f64,
    to: f64,
}

impl Slide {
    pub fn hidden(now: Instant) -> Self {
        Self {
            start: now,
            duration: Duration::ZERO,
            from: 0.0,
            to: 0.0,
        }
    }

    pub fn value(&self, now: Instant) -> f64 {
        if self.duration.is_zero() {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f64()
            / self.duration.as_secs_f64())
        .clamp(0.0, 1.0);
        // Smoothstep is continuous when retargeted, with no overshoot.
        let eased = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * eased
    }

    pub fn retarget(&mut self, visible: bool, duration: Duration, now: Instant) {
        let from = self.value(now);
        let to: f64 = if visible { 1.0 } else { 0.0 };
        *self = Self {
            start: now,
            duration: duration.mul_f64((to - from).abs()),
            from,
            to,
        };
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.start) < self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_stays_on_selected_display_for_every_edge_and_animation_frame() {
        // Negative origins and unequal monitor heights catch Cocoa/physical
        // coordinate mixups and animations leaking onto adjacent displays.
        for screen in [
            NSRect::new(NSPoint::new(-1728.0, 54.0), NSSize::new(1728.0, 1017.0)),
            NSRect::new(NSPoint::new(0.0, -1440.0), NSSize::new(2560.0, 1375.0)),
        ] {
            for edge in [
                QuickTerminalPosition::Top,
                QuickTerminalPosition::Bottom,
                QuickTerminalPosition::Left,
                QuickTerminalPosition::Right,
            ] {
                let config = MacosQuickTerminalConfig {
                    position: edge,
                    width: 0.75,
                    height: 0.4,
                    ..Default::default()
                };
                let target = target_frame(screen, &config);
                for step in 0..=100 {
                    let frame = revealed_frame(target, edge, step as f64 / 100.0);
                    assert!(frame.origin.x >= screen.origin.x);
                    assert!(frame.origin.y >= screen.origin.y);
                    assert!(
                        frame.origin.x + frame.size.width
                            <= screen.origin.x + screen.size.width + 1e-9
                    );
                    assert!(
                        frame.origin.y + frame.size.height
                            <= screen.origin.y + screen.size.height + 1e-9
                    );
                }
                assert_eq!(revealed_frame(target, edge, 1.0), target);
            }
        }
    }

    #[test]
    fn toggling_mid_animation_reverses_without_jumping() {
        let now = Instant::now();
        let duration = Duration::from_millis(200);
        let mut slide = Slide::hidden(now);
        slide.retarget(true, duration, now);
        let halfway = now + duration / 2;
        let visible = slide.value(halfway);
        assert!((visible - 0.5).abs() < 1e-9);
        slide.retarget(false, duration, halfway);
        assert_eq!(slide.value(halfway), visible);
        assert!(slide.value(halfway + Duration::from_millis(50)) < visible);
        assert_eq!(slide.value(halfway + duration), 0.0);
        assert!(!slide.animating(halfway + duration));
    }

    #[test]
    fn reduced_motion_and_zero_duration_complete_immediately() {
        let now = Instant::now();
        let mut slide = Slide::hidden(now);
        slide.retarget(true, Duration::ZERO, now);
        assert_eq!(slide.value(now), 1.0);
        assert!(!slide.animating(now));
        slide.retarget(false, Duration::ZERO, now);
        assert_eq!(slide.value(now), 0.0);
    }
}
