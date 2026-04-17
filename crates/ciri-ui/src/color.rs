//! Colors, as linear-RGBA premultiplied-on-upload `[f32; 4]` tuples.
//!
//! The GPU layer (`ciri-gpu`) expects linear RGBA; hex is sRGB. Always go
//! through `from_srgb_hex` or `from_srgb_arr` when the input is sRGB.

/// Linear RGBA in `[0.0, 1.0]`. Alpha is straight, not pre-multiplied.
pub type Color = [f32; 4];

/// Parse `#rrggbb` (sRGB) → linear RGBA. Invalid input falls back to a
/// bright magenta so misses are obvious on screen rather than silent.
pub fn from_srgb_hex(hex: &str) -> Color {
    ciri_config::theme::ThemeConfig::parse_color_linear(hex)
}

/// Return `c` with its alpha replaced by `a`.
pub const fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], a]
}

/// Return `c` with its alpha multiplied by `mul`.
pub fn mul_alpha(c: Color, mul: f32) -> Color {
    [c[0], c[1], c[2], c[3] * mul]
}

/// Scale the RGB channels by `k` (alpha untouched). Useful for cheap
/// "slightly darker" / "slightly lighter" variants without a full HSL round-trip.
pub fn scale_rgb(c: Color, k: f32) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// Fully transparent sentinel.
pub const TRANSPARENT: Color = [0.0, 0.0, 0.0, 0.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_alpha_replaces() {
        assert_eq!(with_alpha([0.2, 0.4, 0.6, 1.0], 0.5), [0.2, 0.4, 0.6, 0.5]);
    }

    #[test]
    fn mul_alpha_scales() {
        let c = mul_alpha([0.2, 0.4, 0.6, 0.8], 0.5);
        assert!((c[3] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn scale_rgb_leaves_alpha() {
        let c = scale_rgb([0.4, 0.4, 0.4, 0.75], 0.5);
        assert_eq!(c, [0.2, 0.2, 0.2, 0.75]);
    }
}
