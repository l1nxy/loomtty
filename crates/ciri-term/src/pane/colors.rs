use alacritty_terminal::vte::ansi::Rgb;

#[derive(Debug, Clone)]
pub struct TerminalColors {
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self {
            ansi: [
                Rgb { r: 0, g: 0, b: 0 },
                Rgb { r: 205, g: 0, b: 0 },
                Rgb { r: 0, g: 205, b: 0 },
                Rgb { r: 205, g: 205, b: 0 },
                Rgb { r: 0, g: 0, b: 238 },
                Rgb { r: 205, g: 0, b: 205 },
                Rgb { r: 0, g: 205, b: 205 },
                Rgb { r: 229, g: 229, b: 229 },
                Rgb { r: 127, g: 127, b: 127 },
                Rgb { r: 255, g: 0, b: 0 },
                Rgb { r: 0, g: 255, b: 0 },
                Rgb { r: 255, g: 255, b: 0 },
                Rgb { r: 92, g: 92, b: 255 },
                Rgb { r: 255, g: 0, b: 255 },
                Rgb { r: 0, g: 255, b: 255 },
                Rgb { r: 255, g: 255, b: 255 },
            ],
            foreground: Rgb { r: 255, g: 255, b: 255 },
            background: Rgb { r: 0, g: 0, b: 0 },
            cursor: Rgb { r: 255, g: 255, b: 255 },
        }
    }
}

impl TerminalColors {
    pub fn parse_hex(hex: &str) -> Rgb {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            Rgb { r, g, b }
        } else {
            Rgb { r: 255, g: 255, b: 255 }
        }
    }
}

/// Standard xterm default color for a given palette index.
pub(super) fn default_color(index: usize) -> Rgb {
    #[rustfmt::skip]
    const ANSI16: [(u8, u8, u8); 16] = [
        (  0,   0,   0), (205,   0,   0), (  0, 205,   0), (205, 205,   0),
        (  0,   0, 238), (205,   0, 205), (  0, 205, 205), (229, 229, 229),
        (127, 127, 127), (255,   0,   0), (  0, 255,   0), (255, 255,   0),
        ( 92,  92, 255), (255,   0, 255), (  0, 255, 255), (255, 255, 255),
    ];

    if index < 16 {
        let (r, g, b) = ANSI16[index];
        return Rgb { r, g, b };
    }
    if index < 232 {
        let idx = (index - 16) as u8;
        let r = idx / 36;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_component = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
        return Rgb { r: to_component(r), g: to_component(g), b: to_component(b) };
    }
    if index < 256 {
        let v = (8 + 10 * (index - 232)) as u8;
        return Rgb { r: v, g: v, b: v };
    }
    match index {
        256 => Rgb { r: 255, g: 255, b: 255 },
        257 => Rgb { r: 0, g: 0, b: 0 },
        258 => Rgb { r: 255, g: 255, b: 255 },
        _ => Rgb { r: 255, g: 255, b: 255 },
    }
}
