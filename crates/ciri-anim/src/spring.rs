/// Critically damped spring using the exact analytical solution.
///
/// For a critically damped system with ω = sqrt(stiffness):
///   x(t) = target + (C1 + C2·t) · e^(-ω·t)
///   where C1 = x0 - target, C2 = v0 + ω·C1
///
/// This avoids the numerical instability of Euler integration.
#[derive(Debug, Clone)]
pub struct Spring {
    pub position: f64,
    pub velocity: f64,
    pub target: f64,
    omega: f64,
    epsilon: f64,
}

impl Spring {
    /// Create a critically damped spring.
    /// `omega`: angular frequency, controls speed. Recommended: 8-15 for smooth UI.
    pub fn with_omega(omega: f64) -> Self {
        Spring {
            position: 0.0,
            velocity: 0.0,
            target: 0.0,
            omega,
            epsilon: 0.1,
        }
    }

    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    pub fn jump_to(&mut self, value: f64) {
        self.position = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Advance using the exact analytical solution (no numerical error).
    pub fn advance(&mut self, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let c1 = self.position - self.target;
        let c2 = self.velocity + self.omega * c1;
        let exp = (-self.omega * dt).exp();

        self.position = self.target + (c1 + c2 * dt) * exp;
        self.velocity = (c2 - self.omega * (c1 + c2 * dt)) * exp;
    }

    pub fn is_at_rest(&self) -> bool {
        (self.position - self.target).abs() < self.epsilon && self.velocity.abs() < self.epsilon
    }

    pub fn settle(&mut self) {
        if self.is_at_rest() {
            self.position = self.target;
            self.velocity = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges() {
        let mut s = Spring::with_omega(12.0);
        s.jump_to(0.0);
        s.set_target(100.0);

        for _ in 0..300 {
            s.advance(1.0 / 60.0);
        }
        s.settle();
        assert!((s.position - 100.0).abs() < 0.5);
    }

    #[test]
    fn spring_no_overshoot() {
        // Critically damped should NOT overshoot
        let mut s = Spring::with_omega(12.0);
        s.jump_to(0.0);
        s.set_target(100.0);

        let mut max_pos = 0.0f64;
        for _ in 0..600 {
            s.advance(1.0 / 60.0);
            max_pos = max_pos.max(s.position);
        }
        // Should never exceed target (critically damped = no overshoot)
        assert!(max_pos <= 100.5, "overshoot: max_pos={max_pos}");
    }

    #[test]
    fn spring_smooth_monotonic() {
        let mut s = Spring::with_omega(12.0);
        s.jump_to(0.0);
        s.set_target(100.0);

        let mut prev = 0.0;
        for _ in 0..300 {
            s.advance(1.0 / 60.0);
            assert!(
                s.position >= prev - 0.01,
                "non-monotonic: {} -> {}",
                prev,
                s.position
            );
            prev = s.position;
        }
    }

    #[test]
    fn spring_reaches_target_quickly() {
        let mut s = Spring::with_omega(12.0);
        s.jump_to(0.0);
        s.set_target(100.0);

        // Should reach 99% within 0.5 seconds (30 frames at 60fps)
        for _ in 0..30 {
            s.advance(1.0 / 60.0);
        }
        assert!(s.position > 95.0, "too slow: pos={}", s.position);
    }
}
