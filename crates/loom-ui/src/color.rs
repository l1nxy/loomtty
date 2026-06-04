//! Colors, as sRGB-passthrough `[f32; 4]` tuples.
//!
//! To match the legacy `Rect` / glyph rendering pipeline the rest of the
//! client uses, loom-ui keeps color channels in the same sRGB space the
//! hex strings in `ThemeConfig` parse into — no gamma decode, no
//! pre-multiplication. The GPU blend state is configured to treat these
//! values as-is; switching to a linear workflow is a separate project.
//!
//! Alpha is straight (not pre-multiplied). Pre-multiplication, when
//! needed, happens inside the SDF shader.

/// sRGB RGBA in `[0.0, 1.0]`. Alpha is straight.
pub type Color = [f32; 4];

/// Parse `#rrggbb` → sRGB RGBA (no gamma decode). Invalid input falls back
/// to a bright magenta so misses are obvious on screen rather than silent.
pub fn from_srgb_hex(hex: &str) -> Color {
    loom_config::theme::ThemeConfig::parse_color(hex)
}

/// Return `c` with its alpha replaced by `a`.
pub const fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], a]
}

/// Return `c` with its alpha multiplied by `mul`.
pub fn mul_alpha(c: Color, mul: f32) -> Color {
    [c[0], c[1], c[2], c[3] * mul]
}

/// Scale the RGB channels by `k` (alpha untouched) and clamp each to
/// `[0.0, 1.0]`. Useful for cheap "slightly darker" / "slightly lighter"
/// variants without a full HSL round-trip; clamping preserves the `Color`
/// invariant even for light themes where a `k > 1` on a near-white surface
/// would otherwise overflow.
pub fn scale_rgb(c: Color, k: f32) -> Color {
    [
        (c[0] * k).clamp(0.0, 1.0),
        (c[1] * k).clamp(0.0, 1.0),
        (c[2] * k).clamp(0.0, 1.0),
        c[3],
    ]
}

/// Fully transparent sentinel.
pub const TRANSPARENT: Color = [0.0, 0.0, 0.0, 0.0];

/// Pick a near-black or near-white foreground that reads legibly on top of
/// the given sRGB color. Decodes the sRGB channels to linear light first
/// (Rec.709 coefficients), then compares against the WCAG pivot `0.179` —
/// the luminance at which a white foreground and a black foreground give
/// the same contrast ratio. Doing the comparison in linear space keeps
/// saturated mid-brightness accents (olive, brown, deep teal) from
/// collapsing to the wrong branch, which the cheaper sRGB-passthrough
/// formula does. Alpha of `bg` is ignored.
pub fn contrast_on(bg: Color) -> Color {
    fn linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let luma = 0.2126 * linear(bg[0]) + 0.7152 * linear(bg[1]) + 0.0722 * linear(bg[2]);
    if luma > 0.179 {
        [0.05, 0.05, 0.06, 1.0]
    } else {
        [0.95, 0.95, 0.95, 1.0]
    }
}

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

    /// Regression: a light theme (near-white surface) lifted by `k > 1`
    /// must not overflow past 1.0 — GPU paths clamp or distort out-of-range
    /// channels, and derived theme tokens would go wrong silently.
    #[test]
    fn scale_rgb_clamps_high() {
        let c = scale_rgb([1.0, 1.0, 1.0, 1.0], 1.12);
        assert_eq!(c, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn scale_rgb_clamps_low() {
        let c = scale_rgb([0.1, 0.2, 0.3, 1.0], -1.0);
        assert_eq!(c, [0.0, 0.0, 0.0, 1.0]);
    }
}
