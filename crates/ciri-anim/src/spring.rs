/// Spring physics parameters supporting all three damping regimes.
///
/// Based on the damped harmonic oscillator model from libadwaita/niri:
///   m·x'' + b·x' + k·x = 0
///
/// - `damping_ratio < 1.0`: underdamped (bouncy, overshoots target)
/// - `damping_ratio = 1.0`: critically damped (fastest without overshoot)
/// - `damping_ratio > 1.0`: overdamped (slow, no overshoot)
#[derive(Debug, Clone, Copy)]
pub struct SpringParams {
    /// Damping coefficient (computed from damping_ratio and stiffness).
    pub damping: f64,
    /// Mass (always 1.0).
    pub mass: f64,
    /// Spring stiffness.
    pub stiffness: f64,
    /// Convergence threshold.
    pub epsilon: f64,
}

impl SpringParams {
    /// Create spring parameters from a damping ratio and stiffness.
    ///
    /// `damping_ratio`: <1 bouncy, =1 critical, >1 overdamped
    /// `stiffness`: spring constant (higher = snappier)
    /// `epsilon`: convergence threshold for settling detection
    pub fn new(damping_ratio: f64, stiffness: f64, epsilon: f64) -> Self {
        let damping_ratio = damping_ratio.max(0.001);
        let stiffness = stiffness.max(0.001);
        let epsilon = epsilon.max(0.0001);

        let mass = 1.0;
        let critical_damping = 2.0 * (mass * stiffness).sqrt();
        let damping = damping_ratio * critical_damping;

        Self {
            damping,
            mass,
            stiffness,
            epsilon,
        }
    }

    // ── Named presets ──

    /// Fast, no overshoot. Good for view scrolling and focus transitions.
    pub fn snappy() -> Self {
        Self::new(1.0, 1000.0, 0.001)
    }

    /// Smooth, no overshoot. Good for column resizing.
    pub fn smooth() -> Self {
        Self::new(1.0, 500.0, 0.001)
    }

    /// Bouncy with overshoot. Good for pane open, notification pop-in.
    pub fn bouncy() -> Self {
        Self::new(0.6, 800.0, 0.001)
    }

    /// Slow and gentle. Good for subtle background transitions.
    pub fn gentle() -> Self {
        Self::new(0.8, 300.0, 0.01)
    }

    /// Convert from legacy omega + epsilon (critically damped only).
    /// omega² = stiffness / mass, mass = 1.
    pub fn from_omega(omega: f64, epsilon: f64) -> Self {
        Self::new(1.0, omega * omega, epsilon)
    }
}

/// A spring animation from one value to another.
///
/// Uses the exact analytical solution for the damped harmonic oscillator,
/// avoiding numerical instability of Euler integration.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub from: f64,
    pub to: f64,
    pub initial_velocity: f64,
    pub params: SpringParams,
}

impl Spring {
    /// Compute the spring position at time `t` seconds.
    ///
    /// Three-branch analytical solution:
    /// - critically damped: `target + (C1 + C2·t) · e^(-β·t)`
    /// - underdamped: oscillatory with decaying envelope
    /// - overdamped: sum of two exponentials
    pub fn value_at(&self, t: f64) -> f64 {
        let b = self.params.damping;
        let m = self.params.mass;
        let k = self.params.stiffness;
        let v0 = self.initial_velocity;

        let beta = b / (2.0 * m);
        let omega0 = (k / m).sqrt();
        let x0 = self.from - self.to;
        let envelope = (-beta * t).exp();

        // f64::EPSILON is too small for this comparison (from niri/libadwaita)
        if (beta - omega0).abs() <= f64::from(f32::EPSILON) {
            // Critically damped
            self.to + envelope * (x0 + (beta * x0 + v0) * t)
        } else if beta < omega0 {
            // Underdamped
            let omega1 = (omega0 * omega0 - beta * beta).sqrt();
            self.to
                + envelope
                    * (x0 * (omega1 * t).cos()
                        + ((beta * x0 + v0) / omega1) * (omega1 * t).sin())
        } else {
            // Overdamped
            let omega2 = (beta * beta - omega0 * omega0).sqrt();
            self.to
                + envelope
                    * (x0 * (omega2 * t).cosh()
                        + ((beta * x0 + v0) / omega2) * (omega2 * t).sinh())
        }
    }

    /// Compute the velocity at time `t` seconds (analytical derivative).
    pub fn velocity_at(&self, t: f64) -> f64 {
        let b = self.params.damping;
        let m = self.params.mass;
        let k = self.params.stiffness;
        let v0 = self.initial_velocity;

        let beta = b / (2.0 * m);
        let omega0 = (k / m).sqrt();
        let x0 = self.from - self.to;
        let envelope = (-beta * t).exp();

        if (beta - omega0).abs() <= f64::from(f32::EPSILON) {
            // Critically damped: d/dt [target + (C1 + C2·t)·e^(-β·t)]
            let c1 = x0;
            let c2 = v0 + beta * x0;
            envelope * (c2 - beta * (c1 + c2 * t))
        } else if beta < omega0 {
            // Underdamped
            let omega1 = (omega0 * omega0 - beta * beta).sqrt();
            let a = x0;
            let b_coeff = (beta * x0 + v0) / omega1;
            envelope
                * ((-beta * a + omega1 * b_coeff) * (omega1 * t).cos()
                    + (-beta * b_coeff - omega1 * a) * (omega1 * t).sin())
        } else {
            // Overdamped
            let omega2 = (beta * beta - omega0 * omega0).sqrt();
            let a = x0;
            let b_coeff = (beta * x0 + v0) / omega2;
            envelope
                * ((-beta * a + omega2 * b_coeff) * (omega2 * t).cosh()
                    + (-beta * b_coeff + omega2 * a) * (omega2 * t).sinh())
        }
    }

    /// Compute duration until the spring is at rest.
    ///
    /// Uses Newton's method for overdamped springs (from libadwaita).
    pub fn duration(&self) -> f64 {
        const DELTA: f64 = 0.001;

        let beta = self.params.damping / (2.0 * self.params.mass);

        if beta.abs() <= f64::EPSILON || beta < 0.0 {
            return f64::MAX;
        }

        if (self.to - self.from).abs() <= f64::EPSILON {
            return 0.0;
        }

        let omega0 = (self.params.stiffness / self.params.mass).sqrt();

        // Envelope-based estimate (works for critical/underdamped)
        let x0 = -self.params.epsilon.ln() / beta;

        if (beta - omega0).abs() <= f64::from(f32::EPSILON) || beta < omega0 {
            return x0;
        }

        // Overdamped: refine with Newton's method
        let y0 = self.value_at(x0);
        let m = (self.value_at(x0 + DELTA) - y0) / DELTA;

        if m.abs() <= f64::EPSILON {
            return x0;
        }

        let mut x1 = (self.to - y0 + m * x0) / m;
        if x1 < 0.0 {
            x1 = x0; // fallback to envelope estimate
        }
        let mut y1 = self.value_at(x1);

        for _ in 0..1000 {
            if (self.to - y1).abs() <= self.params.epsilon {
                break;
            }

            let x_prev = x1;
            let y_prev = y1;
            let slope = (self.value_at(x_prev + DELTA) - y_prev) / DELTA;

            if slope.abs() <= f64::EPSILON {
                return x_prev;
            }

            x1 = (self.to - y_prev + slope * x_prev) / slope;
            if x1 < 0.0 || !x1.is_finite() {
                return x_prev;
            }
            y1 = self.value_at(x1);

            if !y1.is_finite() {
                return x_prev;
            }
        }

        x1.max(0.0)
    }

    /// Duration until the spring first reaches its target value.
    /// Returns `None` if it cannot be determined within 3 seconds.
    pub fn clamped_duration(&self) -> Option<f64> {
        let beta = self.params.damping / (2.0 * self.params.mass);

        if beta.abs() <= f64::EPSILON || beta < 0.0 {
            return Some(f64::MAX);
        }

        if (self.to - self.from).abs() <= f64::EPSILON {
            return Some(0.0);
        }

        // Step through at 1ms resolution
        let mut i = 1u32;
        let mut y = self.value_at(f64::from(i) / 1000.0);

        while (self.to - self.from > f64::EPSILON && self.to - y > self.params.epsilon)
            || (self.from - self.to > f64::EPSILON && y - self.to > self.params.epsilon)
        {
            if i > 3000 {
                return None;
            }
            i += 1;
            y = self.value_at(f64::from(i) / 1000.0);
        }

        Some(f64::from(i) / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn critical_spring(from: f64, to: f64) -> Spring {
        Spring {
            from,
            to,
            initial_velocity: 0.0,
            params: SpringParams::snappy(),
        }
    }

    #[test]
    fn critically_damped_converges() {
        let s = critical_spring(0.0, 100.0);
        let val = s.value_at(1.0);
        assert!(
            (val - 100.0).abs() < 0.5,
            "should converge: val={val}"
        );
    }

    #[test]
    fn critically_damped_no_overshoot() {
        let s = critical_spring(0.0, 100.0);
        let mut max = 0.0f64;
        for i in 0..600 {
            let t = i as f64 / 60.0;
            max = max.max(s.value_at(t));
        }
        assert!(max <= 100.5, "overshoot: max={max}");
    }

    #[test]
    fn underdamped_overshoots() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::bouncy(),
        };
        let mut max = 0.0f64;
        for i in 0..600 {
            let t = i as f64 / 60.0;
            max = max.max(s.value_at(t));
        }
        assert!(max > 100.5, "bouncy should overshoot: max={max}");

        // But should still converge
        let final_val = s.value_at(5.0);
        assert!(
            (final_val - 100.0).abs() < 1.0,
            "should converge: val={final_val}"
        );
    }

    #[test]
    fn overdamped_no_overshoot() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::new(2.0, 500.0, 0.001),
        };
        let mut max = 0.0f64;
        for i in 0..600 {
            let t = i as f64 / 60.0;
            max = max.max(s.value_at(t));
        }
        assert!(max <= 100.5, "overdamped should not overshoot: max={max}");
    }

    #[test]
    fn overdamped_slower_than_critical() {
        let critical = critical_spring(0.0, 100.0);
        let overdamped = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::new(2.0, 1000.0, 0.001),
        };

        let t = 0.2;
        let c_val = critical.value_at(t);
        let o_val = overdamped.value_at(t);
        assert!(
            c_val > o_val,
            "critical ({c_val}) should be faster than overdamped ({o_val})"
        );
    }

    #[test]
    fn duration_returns_reasonable_value() {
        let s = critical_spring(0.0, 100.0);
        let dur = s.duration();
        assert!(dur > 0.0 && dur < 10.0, "duration={dur}");
    }

    #[test]
    fn clamped_duration_for_critical() {
        let s = critical_spring(0.0, 100.0);
        let dur = s.clamped_duration();
        assert!(dur.is_some());
        let dur = dur.unwrap();
        assert!(dur > 0.0 && dur < 5.0, "clamped_duration={dur}");
    }

    #[test]
    fn clamped_duration_for_bouncy() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::bouncy(),
        };
        // Bouncy reaches target before fully settling
        let clamped = s.clamped_duration();
        let full = s.duration();
        assert!(clamped.is_some());
        assert!(clamped.unwrap() < full, "clamped < full duration");
    }

    #[test]
    fn from_omega_backward_compat() {
        let params = SpringParams::from_omega(12.0, 0.1);
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params,
        };
        // Should behave like old critically damped spring
        let val = s.value_at(0.5);
        assert!(val > 95.0, "should converge quickly with omega=12: val={val}");
    }

    #[test]
    fn zero_distance_is_instant() {
        let s = Spring {
            from: 50.0,
            to: 50.0,
            initial_velocity: 0.0,
            params: SpringParams::snappy(),
        };
        assert_eq!(s.duration(), 0.0);
        assert_eq!(s.clamped_duration(), Some(0.0));
    }

    #[test]
    fn velocity_at_works() {
        let s = critical_spring(0.0, 100.0);
        // At t=0, v0=0 (starts from rest), velocity is 0
        let v0 = s.velocity_at(0.0);
        assert!(
            v0.abs() < 1.0,
            "velocity at rest start should be ~0: v0={v0}"
        );

        // Shortly after, velocity should be positive (toward target)
        let v_early = s.velocity_at(0.01);
        assert!(v_early > 0.0, "velocity should be positive: v={v_early}");

        let v_late = s.velocity_at(1.0);
        assert!(
            v_late.abs() < v_early.abs(),
            "velocity should decrease: v_early={v_early}, v_late={v_late}"
        );
    }

    #[test]
    fn velocity_at_underdamped() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::bouncy(),
        };
        // Velocity should be positive early (moving toward target)
        let v = s.velocity_at(0.05);
        assert!(v > 0.0, "underdamped velocity should be positive early: {v}");
        // Should eventually oscillate (velocity changes sign)
        let mut sign_changes = 0;
        let mut prev_sign = 1.0f64;
        for i in 1..200 {
            let t = i as f64 * 0.01;
            let v = s.velocity_at(t);
            assert!(v.is_finite(), "velocity should be finite at t={t}");
            if v * prev_sign < 0.0 {
                sign_changes += 1;
            }
            if v.abs() > 0.001 {
                prev_sign = v.signum();
            }
        }
        assert!(sign_changes > 0, "underdamped should oscillate");
    }

    #[test]
    fn velocity_at_overdamped() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::new(2.0, 500.0, 0.001),
        };
        // Velocity should be positive (monotonically approaching target)
        let v_early = s.velocity_at(0.05);
        assert!(v_early > 0.0, "overdamped velocity should be positive: {v_early}");
        // Should decrease over time (no oscillation)
        let v_late = s.velocity_at(1.0);
        assert!(v_late >= 0.0, "overdamped should not go negative: {v_late}");
        assert!(v_late < v_early, "overdamped velocity should decrease");
    }

    #[test]
    fn velocity_at_with_initial_velocity() {
        let s = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 500.0,
            params: SpringParams::snappy(),
        };
        let v0 = s.velocity_at(0.0);
        assert!(
            (v0 - 500.0).abs() < 1.0,
            "velocity at t=0 should match initial_velocity: {v0}"
        );
    }

    #[test]
    fn equal_from_to_overdamped_no_nan() {
        let s = Spring {
            from: 0.0,
            to: 0.0,
            initial_velocity: 0.0,
            params: SpringParams::new(1.15, 850.0, 0.0001),
        };
        let _ = s.duration();
        let _ = s.clamped_duration();
        let v = s.value_at(0.0);
        assert!(v.is_finite());
    }

    #[test]
    fn highly_overdamped_duration_no_panic() {
        let s = Spring {
            from: 0.0,
            to: 1.0,
            initial_velocity: 0.0,
            params: SpringParams::new(6.0, 1200.0, 0.0001),
        };
        let d = s.duration();
        assert!(d.is_finite(), "duration should be finite: {d}");
    }

    #[test]
    fn presets_are_valid() {
        for params in [
            SpringParams::snappy(),
            SpringParams::smooth(),
            SpringParams::bouncy(),
            SpringParams::gentle(),
        ] {
            assert!(params.damping > 0.0);
            assert!(params.stiffness > 0.0);
            assert!(params.epsilon > 0.0);
        }
    }

    #[test]
    fn initial_velocity_affects_trajectory() {
        let s_still = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 0.0,
            params: SpringParams::snappy(),
        };
        let s_fast = Spring {
            from: 0.0,
            to: 100.0,
            initial_velocity: 500.0,
            params: SpringParams::snappy(),
        };

        let t = 0.05;
        assert!(
            s_fast.value_at(t) > s_still.value_at(t),
            "initial velocity should push ahead"
        );
    }
}
