//! Color resolution: ANSI, indexed, named → RGBA.

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use loom_config::config::LoomConfig;
use loom_config::theme::ThemeConfig;
use loom_protocol::message::{COLOR_INDEXED, COLOR_NAMED, COLOR_RGB, PackedColor};

/// Pre-computed color lookup table to avoid per-cell hex string parsing.
pub struct ColorTable {
    pub(super) named: [[f32; 4]; 16],
    pub(super) foreground: [f32; 4],
    pub(super) background: [f32; 4],
    pub(super) dim_foreground: [f32; 4],
    pub(super) dim_colors: [[f32; 4]; 8],
}

impl ColorTable {
    pub fn new(config: &LoomConfig) -> Self {
        let theme = &config.theme;
        let fg = ThemeConfig::parse_color(&theme.foreground);
        let named = [
            ThemeConfig::parse_color(&theme.black),
            ThemeConfig::parse_color(&theme.red),
            ThemeConfig::parse_color(&theme.green),
            ThemeConfig::parse_color(&theme.yellow),
            ThemeConfig::parse_color(&theme.blue),
            ThemeConfig::parse_color(&theme.magenta),
            ThemeConfig::parse_color(&theme.cyan),
            ThemeConfig::parse_color(&theme.white),
            ThemeConfig::parse_color(&theme.bright_black),
            ThemeConfig::parse_color(&theme.bright_red),
            ThemeConfig::parse_color(&theme.bright_green),
            ThemeConfig::parse_color(&theme.bright_yellow),
            ThemeConfig::parse_color(&theme.bright_blue),
            ThemeConfig::parse_color(&theme.bright_magenta),
            ThemeConfig::parse_color(&theme.bright_cyan),
            ThemeConfig::parse_color(&theme.bright_white),
        ];
        let mut dim_colors = [[0.0f32; 4]; 8];
        for i in 0..8 {
            dim_colors[i] = [
                named[i][0] * 0.67,
                named[i][1] * 0.67,
                named[i][2] * 0.67,
                named[i][3],
            ];
        }
        ColorTable {
            named,
            foreground: fg,
            background: ThemeConfig::parse_color(&theme.background),
            dim_foreground: [fg[0] * 0.67, fg[1] * 0.67, fg[2] * 0.67, fg[3]],
            dim_colors,
        }
    }

    pub fn resolve_packed(&self, color: PackedColor) -> [f32; 4] {
        match color.tag {
            COLOR_NAMED => {
                let n = color.b1;
                match n {
                    0..=15 => self.named[n as usize],
                    16 | 27 => self.foreground,
                    17 => self.background,
                    18 => self.foreground,
                    19..=26 => self.dim_colors[(n - 19) as usize],
                    28 => self.dim_foreground,
                    _ => self.foreground,
                }
            }
            COLOR_RGB => [
                color.b1 as f32 / 255.0,
                color.b2 as f32 / 255.0,
                color.b3 as f32 / 255.0,
                1.0,
            ],
            COLOR_INDEXED => indexed_color_to_rgba_table(color.b1, self),
            _ => [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Resolve 256-color index using pre-computed color table.
fn indexed_color_to_rgba_table(idx: u8, ct: &ColorTable) -> [f32; 4] {
    if idx < 16 {
        return ct.named[idx as usize];
    }
    if idx < 232 {
        let i = idx - 16;
        let r = (i / 36) % 6;
        let g = (i / 6) % 6;
        let b = i % 6;
        let to_f = |v: u8| {
            if v == 0 {
                0.0
            } else {
                (55.0 + 40.0 * v as f32) / 255.0
            }
        };
        return [to_f(r), to_f(g), to_f(b), 1.0];
    }
    let v = (8 + 10 * (idx - 232) as u32) as f32 / 255.0;
    [v, v, v, 1.0]
}

// ─── Server-side color resolution ────────────────────────────────────

/// Resolve alacritty `AnsiColor` to RGBA.
pub(super) fn ansi_color_to_rgba(color: AnsiColor, config: &LoomConfig) -> [f32; 4] {
    match color {
        AnsiColor::Named(named) => named_color_to_rgba(named, config),
        AnsiColor::Spec(rgb) => [
            rgb.r as f32 / 255.0,
            rgb.g as f32 / 255.0,
            rgb.b as f32 / 255.0,
            1.0,
        ],
        AnsiColor::Indexed(idx) => indexed_color_to_rgba(idx, config),
    }
}

/// Map named color index (0–15) to alacritty `NamedColor`.
fn named_color_from_index(idx: u8) -> NamedColor {
    match idx {
        0 => NamedColor::Black,
        1 => NamedColor::Red,
        2 => NamedColor::Green,
        3 => NamedColor::Yellow,
        4 => NamedColor::Blue,
        5 => NamedColor::Magenta,
        6 => NamedColor::Cyan,
        7 => NamedColor::White,
        8 => NamedColor::BrightBlack,
        9 => NamedColor::BrightRed,
        10 => NamedColor::BrightGreen,
        11 => NamedColor::BrightYellow,
        12 => NamedColor::BrightBlue,
        13 => NamedColor::BrightMagenta,
        14 => NamedColor::BrightCyan,
        15 => NamedColor::BrightWhite,
        _ => NamedColor::Foreground,
    }
}

/// Resolve alacritty `NamedColor` to RGBA using the theme config.
fn named_color_to_rgba(c: NamedColor, config: &LoomConfig) -> [f32; 4] {
    let theme = &config.theme;
    match c {
        NamedColor::Black => ThemeConfig::parse_color(&theme.black),
        NamedColor::Red => ThemeConfig::parse_color(&theme.red),
        NamedColor::Green => ThemeConfig::parse_color(&theme.green),
        NamedColor::Yellow => ThemeConfig::parse_color(&theme.yellow),
        NamedColor::Blue => ThemeConfig::parse_color(&theme.blue),
        NamedColor::Magenta => ThemeConfig::parse_color(&theme.magenta),
        NamedColor::Cyan => ThemeConfig::parse_color(&theme.cyan),
        NamedColor::White => ThemeConfig::parse_color(&theme.white),
        NamedColor::BrightBlack => ThemeConfig::parse_color(&theme.bright_black),
        NamedColor::BrightRed => ThemeConfig::parse_color(&theme.bright_red),
        NamedColor::BrightGreen => ThemeConfig::parse_color(&theme.bright_green),
        NamedColor::BrightYellow => ThemeConfig::parse_color(&theme.bright_yellow),
        NamedColor::BrightBlue => ThemeConfig::parse_color(&theme.bright_blue),
        NamedColor::BrightMagenta => ThemeConfig::parse_color(&theme.bright_magenta),
        NamedColor::BrightCyan => ThemeConfig::parse_color(&theme.bright_cyan),
        NamedColor::BrightWhite => ThemeConfig::parse_color(&theme.bright_white),
        NamedColor::Foreground => ThemeConfig::parse_color(&theme.foreground),
        NamedColor::Background => ThemeConfig::parse_color(&theme.background),
        _ => ThemeConfig::parse_color(&theme.foreground),
    }
}

/// Resolve 256-color index to RGBA.
/// 0–15: named colors, 16–231: 6×6×6 RGB cube, 232–255: grayscale ramp.
fn indexed_color_to_rgba(idx: u8, config: &LoomConfig) -> [f32; 4] {
    if idx < 16 {
        return named_color_to_rgba(named_color_from_index(idx), config);
    }
    if idx < 232 {
        let i = idx - 16;
        let r = (i / 36) % 6;
        let g = (i / 6) % 6;
        let b = i % 6;
        let to_f = |v: u8| {
            if v == 0 {
                0.0
            } else {
                (55.0 + 40.0 * v as f32) / 255.0
            }
        };
        return [to_f(r), to_f(g), to_f(b), 1.0];
    }
    let v = (8 + 10 * (idx - 232) as u32) as f32 / 255.0;
    [v, v, v, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> LoomConfig {
        let mut config = LoomConfig::default();
        config.theme.resolve_preset();
        config
    }

    fn test_color_table() -> ColorTable {
        ColorTable::new(&test_config())
    }

    // ── ColorTable construction ─────────────��────────────────────────

    #[test]
    fn color_table_has_16_named_colors() {
        let ct = test_color_table();
        // All 16 named colors should be non-zero (not all black)
        let non_black = ct
            .named
            .iter()
            .filter(|c| c[0] > 0.0 || c[1] > 0.0 || c[2] > 0.0)
            .count();
        assert!(non_black >= 8, "at least bright colors should be non-black");
    }

    #[test]
    fn color_table_foreground_background_differ() {
        let ct = test_color_table();
        assert_ne!(ct.foreground, ct.background);
    }

    #[test]
    fn dim_colors_are_67_percent_of_named() {
        let ct = test_color_table();
        for i in 0..8 {
            for ch in 0..3 {
                let expected = ct.named[i][ch] * 0.67;
                assert!(
                    (ct.dim_colors[i][ch] - expected).abs() < 1e-5,
                    "dim_colors[{i}][{ch}]: {:.4} != {:.4}",
                    ct.dim_colors[i][ch],
                    expected
                );
            }
            // Alpha preserved
            assert_eq!(ct.dim_colors[i][3], ct.named[i][3]);
        }
    }

    #[test]
    fn dim_foreground_is_67_percent_of_foreground() {
        let ct = test_color_table();
        for ch in 0..3 {
            let expected = ct.foreground[ch] * 0.67;
            assert!(
                (ct.dim_foreground[ch] - expected).abs() < 1e-5,
                "ch {ch}: {:.4} != {:.4}",
                ct.dim_foreground[ch],
                expected
            );
        }
    }

    // ── resolve_packed ────────────��─────────────────────────────────���

    #[test]
    fn resolve_packed_rgb_color() {
        let ct = test_color_table();
        let color = PackedColor {
            tag: COLOR_RGB,
            b1: 255,
            b2: 128,
            b3: 0,
        };
        let result = ct.resolve_packed(color);
        assert_eq!(result[0], 1.0);
        assert!((result[1] - 128.0 / 255.0).abs() < 1e-5);
        assert_eq!(result[2], 0.0);
        assert_eq!(result[3], 1.0);
    }

    #[test]
    fn resolve_packed_named_0_through_15() {
        let ct = test_color_table();
        for i in 0..16u8 {
            let color = PackedColor {
                tag: COLOR_NAMED,
                b1: i,
                b2: 0,
                b3: 0,
            };
            assert_eq!(ct.resolve_packed(color), ct.named[i as usize]);
        }
    }

    #[test]
    fn resolve_packed_named_foreground_variants() {
        let ct = test_color_table();
        // Named index 16 and 27 both map to foreground
        for idx in [16u8, 27] {
            let color = PackedColor {
                tag: COLOR_NAMED,
                b1: idx,
                b2: 0,
                b3: 0,
            };
            assert_eq!(ct.resolve_packed(color), ct.foreground);
        }
    }

    #[test]
    fn resolve_packed_named_background() {
        let ct = test_color_table();
        let color = PackedColor {
            tag: COLOR_NAMED,
            b1: 17,
            b2: 0,
            b3: 0,
        };
        assert_eq!(ct.resolve_packed(color), ct.background);
    }

    #[test]
    fn resolve_packed_named_dim_colors() {
        let ct = test_color_table();
        for i in 0..8u8 {
            let color = PackedColor {
                tag: COLOR_NAMED,
                b1: 19 + i,
                b2: 0,
                b3: 0,
            };
            assert_eq!(ct.resolve_packed(color), ct.dim_colors[i as usize]);
        }
    }

    #[test]
    fn resolve_packed_named_dim_foreground() {
        let ct = test_color_table();
        let color = PackedColor {
            tag: COLOR_NAMED,
            b1: 28,
            b2: 0,
            b3: 0,
        };
        assert_eq!(ct.resolve_packed(color), ct.dim_foreground);
    }

    #[test]
    fn resolve_packed_unknown_tag_returns_white() {
        let ct = test_color_table();
        let color = PackedColor {
            tag: 255,
            b1: 0,
            b2: 0,
            b3: 0,
        };
        assert_eq!(ct.resolve_packed(color), [1.0, 1.0, 1.0, 1.0]);
    }

    // ─��� 256-color indexed resolution ────────────────────��───────────

    #[test]
    fn indexed_color_first_16_match_named() {
        let ct = test_color_table();
        for i in 0..16u8 {
            let color = PackedColor {
                tag: COLOR_INDEXED,
                b1: i,
                b2: 0,
                b3: 0,
            };
            assert_eq!(ct.resolve_packed(color), ct.named[i as usize]);
        }
    }

    #[test]
    fn indexed_color_216_cube_corners() {
        let ct = test_color_table();
        // Index 16 = RGB(0,0,0) → all zeros
        let black = ct.resolve_packed(PackedColor {
            tag: COLOR_INDEXED,
            b1: 16,
            b2: 0,
            b3: 0,
        });
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);

        // Index 231 = RGB(5,5,5) → brightest in cube
        let white = ct.resolve_packed(PackedColor {
            tag: COLOR_INDEXED,
            b1: 231,
            b2: 0,
            b3: 0,
        });
        let max_val = (55.0 + 40.0 * 5.0) / 255.0;
        assert!((white[0] - max_val).abs() < 1e-5);
        assert!((white[1] - max_val).abs() < 1e-5);
        assert!((white[2] - max_val).abs() < 1e-5);

        // Index 196 = RGB(5,0,0) → pure red in cube
        let red = ct.resolve_packed(PackedColor {
            tag: COLOR_INDEXED,
            b1: 196,
            b2: 0,
            b3: 0,
        });
        assert!((red[0] - max_val).abs() < 1e-5);
        assert_eq!(red[1], 0.0);
        assert_eq!(red[2], 0.0);
    }

    #[test]
    fn indexed_color_grayscale_ramp() {
        let ct = test_color_table();
        // Index 232 = first grayscale (darkest)
        let darkest = ct.resolve_packed(PackedColor {
            tag: COLOR_INDEXED,
            b1: 232,
            b2: 0,
            b3: 0,
        });
        let expected = 8.0 / 255.0;
        assert!((darkest[0] - expected).abs() < 1e-5);
        assert_eq!(darkest[0], darkest[1]); // grayscale: r=g=b
        assert_eq!(darkest[1], darkest[2]);

        // Index 255 = last grayscale (brightest)
        let brightest = ct.resolve_packed(PackedColor {
            tag: COLOR_INDEXED,
            b1: 255,
            b2: 0,
            b3: 0,
        });
        let expected = (8 + 10 * 23) as f32 / 255.0;
        assert!((brightest[0] - expected).abs() < 1e-5);

        // Grayscale ramp must be monotonically increasing
        for i in 232..255u8 {
            let c1 = ct.resolve_packed(PackedColor {
                tag: COLOR_INDEXED,
                b1: i,
                b2: 0,
                b3: 0,
            });
            let c2 = ct.resolve_packed(PackedColor {
                tag: COLOR_INDEXED,
                b1: i + 1,
                b2: 0,
                b3: 0,
            });
            assert!(c2[0] > c1[0], "grayscale[{}] >= grayscale[{}]", i + 1, i);
        }
    }
}
