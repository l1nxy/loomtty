//! Emit a JSON fixture file capturing the SM-encoded byte sequence and
//! the expected decoded cells for a curated set of scenarios. The TS
//! decoder loads this file and round-trips it as the ground truth.
//!
//! ```sh
//! cargo run -p loom-protocol --example sm_fixtures > \
//!     web/packages/loom-codec/src/__fixtures__/sm.json
//! ```

use loom_protocol::codec::StateEncoder;
use loom_protocol::message::*;

fn main() {
    let mut cases: Vec<String> = Vec::new();

    cases.push(case(
        "ascii_run",
        &cells_from(|c| {
            for ch in "hello, world!".chars() {
                c.push(plain(ch));
            }
        }),
    ));

    cases.push(case(
        "repeated_space_run",
        &cells_from(|c| {
            for _ in 0..32 {
                c.push(plain(' '));
            }
        }),
    ));

    cases.push(case(
        "mixed_colors",
        &cells_from(|c| {
            c.push(coloured('R', red(), DEFAULT_BACKGROUND));
            c.push(coloured('G', green(), DEFAULT_BACKGROUND));
            c.push(coloured('B', blue(), DEFAULT_BACKGROUND));
        }),
    ));

    cases.push(case(
        "flag_runs",
        &cells_from(|c| {
            c.push(with_flags('a', FLAG_BOLD));
            c.push(with_flags('b', FLAG_BOLD | FLAG_UNDERLINE));
            c.push(plain('c'));
        }),
    ));

    cases.push(case(
        "multibyte",
        &cells_from(|c| {
            c.push(plain('中'));
            c.push(plain('文'));
            c.push(plain('\u{1F600}'));
        }),
    ));

    cases.push(case(
        "cjk_repeats",
        &cells_from(|c| {
            for _ in 0..10 {
                c.push(plain('日'));
            }
        }),
    ));

    cases.push(case(
        "rgb_colors",
        &cells_from(|c| {
            let orange = PackedColor::rgb(0xff, 0x88, 0x00);
            c.push(coloured('x', orange, DEFAULT_BACKGROUND));
        }),
    ));

    // Exercise OP_SET_FG_BG: the encoder only emits that opcode when
    // both fg and bg change in the same step and BOTH are RGB. A
    // single coloured cell here is enough — the encoder's first
    // push() sees fg/bg differ from defaults simultaneously and emits
    // SET_FG_BG rather than two SET_FG / SET_BG opcodes.
    cases.push(case(
        "rgb_fg_and_bg",
        &cells_from(|c| {
            let fg = PackedColor::rgb(0xff, 0x88, 0x00);
            let bg = PackedColor::rgb(0x11, 0x22, 0x33);
            c.push(coloured('y', fg, bg));
        }),
    ));

    println!("[\n{}\n]", cases.join(",\n"));
}

fn case(name: &str, cells: &[PackedCell]) -> String {
    let mut enc = StateEncoder::new();
    for cell in cells {
        enc.push_cell(cell);
    }
    let bytes = enc.finish();
    let hex = bytes_to_hex(bytes);
    let cells_json = cells
        .iter()
        .map(cell_to_json)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "  {{\"name\": {n}, \"sm_bytes_hex\": \"{hex}\", \"expected_cells\": [{cells_json}]}}",
        n = json_string(name),
    )
}

fn cell_to_json(cell: &PackedCell) -> String {
    let ch = cell.ch();
    let fg = color_to_json(&cell.fg);
    let bg = color_to_json(&cell.bg);
    let flags = cell.flags_u16();
    format!(
        "{{\"ch\": {ch_str}, \"fg\": {fg}, \"bg\": {bg}, \"flags\": {flags}}}",
        ch_str = json_string(&ch.to_string()),
    )
}

fn color_to_json(c: &PackedColor) -> String {
    match c.tag {
        COLOR_NAMED => format!("[\"named\", {}]", c.b1),
        COLOR_INDEXED => format!("[\"indexed\", {}]", c.b1),
        COLOR_RGB => format!("[\"rgb\", {}, {}, {}]", c.b1, c.b2, c.b3),
        other => panic!("unknown color tag {other}"),
    }
}

fn bytes_to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for &byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn cells_from(build: impl FnOnce(&mut Vec<PackedCell>)) -> Vec<PackedCell> {
    let mut v = Vec::new();
    build(&mut v);
    v
}

fn plain(ch: char) -> PackedCell {
    PackedCell::with_ch(ch)
}

fn coloured(ch: char, fg: PackedColor, bg: PackedColor) -> PackedCell {
    let mut cell = PackedCell::default();
    cell.set_ch(ch);
    cell.fg = fg;
    cell.bg = bg;
    cell
}

fn with_flags(ch: char, flags: u16) -> PackedCell {
    let mut cell = PackedCell::with_ch(ch);
    cell.flags = flags.to_le_bytes();
    cell
}

fn red() -> PackedColor {
    PackedColor::named(NAMED_RED)
}
fn green() -> PackedColor {
    PackedColor::named(NAMED_GREEN)
}
fn blue() -> PackedColor {
    PackedColor::named(NAMED_BLUE)
}
