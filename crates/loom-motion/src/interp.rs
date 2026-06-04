//! Interpolation trait for animatable values.
//!
//! Implemented for every type that can appear in an animated style field:
//! scalars, 2D offsets, colors, and corner-radius 4-tuples.

pub trait Lerp: Copy + PartialEq + 'static {
    /// Linearly interpolate between `a` and `b` by factor `t` (usually in `[0, 1]`).
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: f32, b: f32, t: f32) -> f32 {
        a + (b - a) * t
    }
}

impl Lerp for [f32; 2] {
    fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
        [f32::lerp(a[0], b[0], t), f32::lerp(a[1], b[1], t)]
    }
}

impl Lerp for [f32; 4] {
    fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
        std::array::from_fn(|i| f32::lerp(a[i], b[i], t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_midpoint() {
        assert!((f32::lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn color_midpoint() {
        let c = <[f32; 4]>::lerp([0.0; 4], [1.0; 4], 0.5);
        assert_eq!(c, [0.5; 4]);
    }

    #[test]
    fn offset_endpoints() {
        let a = [0.0, 0.0];
        let b = [10.0, -4.0];
        assert_eq!(<[f32; 2]>::lerp(a, b, 0.0), a);
        assert_eq!(<[f32; 2]>::lerp(a, b, 1.0), b);
    }
}
