//! Built-in vector rendering for U+2500..U+259F (Box Drawing + Block
//! Elements). These codepoints are designed by font authors to extend
//! beyond the cell box so adjacent cells join seamlessly; rendering them
//! through a per-cell glyph atlas produces overlap brightening at joints
//! and pixel-misaligned corners (the same approach Windows Terminal and
//! Ghostty take in `BuiltinGlyphs.cpp` / `font/sprite/draw/box.zig`
//! respectively — bypass the font and rasterize to cell geometry).
//!
//! Coverage:
//! - Light / heavy / double straight lines, corners, T-junctions, crosses
//! - Light / heavy half-lines (`╴╵╶╷╸╹╺╻`)
//! - Mixed-weight transitions (`╼╽╾╿`)
//! - Curved corners (`╭╮╯╰`) — drawn as right angles
//! - Dashed / dotted lines (`┄┅┆┇┈┉┊┋╌╍╎╏`)
//! - Block elements (full / half / N-eighths / quadrants / shading)
//!
//! Diagonals (`╱╲╳`) are intentionally NOT covered yet — they need
//! anti-aliased rasterization, which the bg-rect pipeline can't do.
//! Those fall through to the font path.

use crate::rect::Rect;

/// Returns true if `ch` is in the built-in range. Always returning true
/// here would be a lie — within `0x2500..=0x259F` we still leave a few
/// holes (diagonals) for the font to handle. `emit` is the source of
/// truth: it returns false when nothing was drawn so the caller can fall
/// back to the existing glyph path.
#[inline]
pub(super) fn is_in_range(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F)
}

/// Emit `ch` as a list of `Rect`s into `out`. Returns true if `ch` was
/// handled (caller should skip the font/atlas path); false if the
/// codepoint was not implemented and the caller should fall back.
///
/// `(x, y)` is the cell's top-left in pixels; `(w, h)` is the cell size.
pub(super) fn emit(
    ch: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    out: &mut Vec<Rect>,
) -> bool {
    let cp = ch as u32;
    if !(0x2500..=0x259F).contains(&cp) {
        return false;
    }
    let m = Metrics::for_cell(w, h);

    // Try the line/junction table first — covers the bulk of U+2500..U+254B.
    if let Some(spec) = line_spec(cp) {
        draw_lines(spec, &m, x, y, color, out);
        return true;
    }

    // Half-lines and mixed-weight transitions.
    if let Some(()) = draw_half_or_transition(cp, &m, x, y, color, out) {
        return true;
    }

    // Dashed / dotted variants.
    if let Some(()) = draw_dashed(cp, &m, x, y, color, out) {
        return true;
    }

    // Curved corners — approximate as right angles.
    if let Some(spec) = curved_corner_spec(cp) {
        draw_lines(spec, &m, x, y, color, out);
        return true;
    }

    // Block elements (U+2580..U+259F).
    if (0x2580..=0x259F).contains(&cp) {
        draw_block_element(cp, x, y, w, h, color, out);
        return true;
    }

    // Diagonals U+2571..U+2573 and any other holes — defer to font.
    false
}

// ─── Internal model ──────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum LineStyle {
    None,
    Light,
    Heavy,
    Double,
}
use LineStyle::{Double, Heavy, Light, None as L0};

#[derive(Copy, Clone, Debug)]
struct LineSpec {
    up: LineStyle,
    right: LineStyle,
    down: LineStyle,
    left: LineStyle,
}

const fn ls(up: LineStyle, right: LineStyle, down: LineStyle, left: LineStyle) -> LineSpec {
    LineSpec {
        up,
        right,
        down,
        left,
    }
}

/// Per-cell metrics used by all draw helpers. Thicknesses scale with the
/// cell height so large fonts get correspondingly thick lines.
struct Metrics {
    cell_w: f32,
    cell_h: f32,
    /// Thickness of a "light" line in pixels.
    light: f32,
    /// Thickness of a "heavy" line in pixels.
    heavy: f32,
    /// Thickness of one stroke of a "double" line. Two strokes plus a gap
    /// of the same width compose a double line — total = 3 * `double`.
    double: f32,
    /// Pre-rounded x-center of a vertical light line.
    cx_light: f32,
    /// Pre-rounded y-center of a horizontal light line.
    cy_light: f32,
    /// Pre-rounded x-center of a vertical heavy line.
    cx_heavy: f32,
    /// Pre-rounded y-center of a horizontal heavy line.
    cy_heavy: f32,
}

impl Metrics {
    fn for_cell(w: f32, h: f32) -> Self {
        let light = ((h / 16.0).round()).max(1.0);
        let heavy = (light * 2.0).max(2.0);
        let double = light;
        let cx_light = ((w - light) * 0.5).round();
        let cy_light = ((h - light) * 0.5).round();
        let cx_heavy = ((w - heavy) * 0.5).round();
        let cy_heavy = ((h - heavy) * 0.5).round();
        Self {
            cell_w: w,
            cell_h: h,
            light,
            heavy,
            double,
            cx_light,
            cy_light,
            cx_heavy,
            cy_heavy,
        }
    }

    fn thick(&self, s: LineStyle) -> f32 {
        match s {
            L0 => 0.0,
            Light | Double => self.light,
            Heavy => self.heavy,
        }
    }
}

#[inline]
fn push(out: &mut Vec<Rect>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    if w > 0.0 && h > 0.0 {
        out.push(Rect { x, y, w, h, color });
    }
}

// ─── Lines + junctions ───────────────────────────────────────────────

fn draw_lines(spec: LineSpec, m: &Metrics, x: f32, y: f32, color: [f32; 4], out: &mut Vec<Rect>) {
    // Horizontal arm spans from `x` to a join column equal to half the
    // cell plus enough padding to cover both this side's stroke and the
    // perpendicular stroke at the centre. The join semantics: the
    // horizontal arm always reaches the centre of the cross-stroke + half
    // of its own stroke, so corners come out flush regardless of the
    // perpendicular weight.
    let cell_mid_x = m.cell_w * 0.5;
    let cell_mid_y = m.cell_h * 0.5;

    // Helper: width of the perpendicular stroke at the join.
    let cross_w = |s: LineStyle| -> f32 { m.thick(s) };

    // ── Left arm ─────────────────────────────────────────────────
    match spec.left {
        L0 => {}
        Light | Heavy => {
            let t = m.thick(spec.left);
            let cy = ((m.cell_h - t) * 0.5).round();
            // Join column extends from x=0 up to cell_mid_x + half the
            // perpendicular stroke (so the corner is square).
            let perp = cross_w(spec.up).max(cross_w(spec.down));
            let end = (cell_mid_x + perp * 0.5).round().min(m.cell_w);
            push(out, x, y + cy, end, t, color);
        }
        Double => {
            // Two strokes; how they meet a perpendicular depends on the
            // perpendicular's weight. For a clean implementation we just
            // draw two parallel thin lines without trying to break at the
            // join — the visual is fine for monospace use.
            let t = m.double;
            let gap = m.double;
            let cy = ((m.cell_h - (3.0 * t)) * 0.5).round();
            let end = (cell_mid_x + (cross_w(spec.up).max(cross_w(spec.down))) * 0.5)
                .round()
                .min(m.cell_w);
            push(out, x, y + cy, end, t, color);
            push(out, x, y + cy + t + gap, end, t, color);
        }
    }

    // ── Right arm ────────────────────────────────────────────────
    match spec.right {
        L0 => {}
        Light | Heavy => {
            let t = m.thick(spec.right);
            let cy = ((m.cell_h - t) * 0.5).round();
            let perp = cross_w(spec.up).max(cross_w(spec.down));
            let start = (cell_mid_x - perp * 0.5).round().max(0.0);
            let end = m.cell_w;
            push(out, x + start, y + cy, end - start, t, color);
        }
        Double => {
            let t = m.double;
            let gap = m.double;
            let cy = ((m.cell_h - (3.0 * t)) * 0.5).round();
            let start = (cell_mid_x - (cross_w(spec.up).max(cross_w(spec.down))) * 0.5)
                .round()
                .max(0.0);
            let end = m.cell_w;
            push(out, x + start, y + cy, end - start, t, color);
            push(out, x + start, y + cy + t + gap, end - start, t, color);
        }
    }

    // ── Up arm ───────────────────────────────────────────────────
    match spec.up {
        L0 => {}
        Light | Heavy => {
            let t = m.thick(spec.up);
            let cx = ((m.cell_w - t) * 0.5).round();
            let perp = cross_w(spec.left).max(cross_w(spec.right));
            let end = (cell_mid_y + perp * 0.5).round().min(m.cell_h);
            push(out, x + cx, y, t, end, color);
        }
        Double => {
            let t = m.double;
            let gap = m.double;
            let cx = ((m.cell_w - (3.0 * t)) * 0.5).round();
            let end = (cell_mid_y + (cross_w(spec.left).max(cross_w(spec.right))) * 0.5)
                .round()
                .min(m.cell_h);
            push(out, x + cx, y, t, end, color);
            push(out, x + cx + t + gap, y, t, end, color);
        }
    }

    // ── Down arm ─────────────────────────────────────────────────
    match spec.down {
        L0 => {}
        Light | Heavy => {
            let t = m.thick(spec.down);
            let cx = ((m.cell_w - t) * 0.5).round();
            let perp = cross_w(spec.left).max(cross_w(spec.right));
            let start = (cell_mid_y - perp * 0.5).round().max(0.0);
            let end = m.cell_h;
            push(out, x + cx, y + start, t, end - start, color);
        }
        Double => {
            let t = m.double;
            let gap = m.double;
            let cx = ((m.cell_w - (3.0 * t)) * 0.5).round();
            let start = (cell_mid_y - (cross_w(spec.left).max(cross_w(spec.right))) * 0.5)
                .round()
                .max(0.0);
            let end = m.cell_h;
            push(out, x + cx, y + start, t, end - start, color);
            push(out, x + cx + t + gap, y + start, t, end - start, color);
        }
    }
    // Suppress unused warnings for the centres — they're used inside the
    // arms via the local computations above.
    let _ = (m.cx_light, m.cy_light, m.cx_heavy, m.cy_heavy);
}

/// Codepoint → 4-edge style. Covers U+2500..U+254B.
const fn line_spec(cp: u32) -> Option<LineSpec> {
    let s = match cp {
        // ── Pure horizontals & verticals ─────────────────────────
        0x2500 => ls(L0, Light, L0, Light),    // ─
        0x2501 => ls(L0, Heavy, L0, Heavy),    // ━
        0x2502 => ls(Light, L0, Light, L0),    // │
        0x2503 => ls(Heavy, L0, Heavy, L0),    // ┃

        // ── Right-angle corners (light/light) ────────────────────
        0x250C => ls(L0, Light, Light, L0),    // ┌
        0x250D => ls(L0, Heavy, Light, L0),    // ┍
        0x250E => ls(L0, Light, Heavy, L0),    // ┎
        0x250F => ls(L0, Heavy, Heavy, L0),    // ┏
        0x2510 => ls(L0, L0, Light, Light),    // ┐
        0x2511 => ls(L0, L0, Light, Heavy),    // ┑
        0x2512 => ls(L0, L0, Heavy, Light),    // ┒
        0x2513 => ls(L0, L0, Heavy, Heavy),    // ┓
        0x2514 => ls(Light, Light, L0, L0),    // └
        0x2515 => ls(Light, Heavy, L0, L0),    // ┕
        0x2516 => ls(Heavy, Light, L0, L0),    // ┖
        0x2517 => ls(Heavy, Heavy, L0, L0),    // ┗
        0x2518 => ls(Light, L0, L0, Light),    // ┘
        0x2519 => ls(Light, L0, L0, Heavy),    // ┙
        0x251A => ls(Heavy, L0, L0, Light),    // ┚
        0x251B => ls(Heavy, L0, L0, Heavy),    // ┛

        // ── Vertical T-junctions (left bar) ──────────────────────
        0x251C => ls(Light, Light, Light, L0), // ├
        0x251D => ls(Light, Heavy, Light, L0), // ┝
        0x251E => ls(Heavy, Light, Light, L0), // ┞
        0x251F => ls(Light, Light, Heavy, L0), // ┟
        0x2520 => ls(Heavy, Light, Heavy, L0), // ┠
        0x2521 => ls(Heavy, Heavy, Light, L0), // ┡
        0x2522 => ls(Light, Heavy, Heavy, L0), // ┢
        0x2523 => ls(Heavy, Heavy, Heavy, L0), // ┣

        // ── Vertical T-junctions (right bar) ─────────────────────
        0x2524 => ls(Light, L0, Light, Light), // ┤
        0x2525 => ls(Light, L0, Light, Heavy), // ┥
        0x2526 => ls(Heavy, L0, Light, Light), // ┦
        0x2527 => ls(Light, L0, Heavy, Light), // ┧
        0x2528 => ls(Heavy, L0, Heavy, Light), // ┨
        0x2529 => ls(Heavy, L0, Light, Heavy), // ┩
        0x252A => ls(Light, L0, Heavy, Heavy), // ┪
        0x252B => ls(Heavy, L0, Heavy, Heavy), // ┫

        // ── Horizontal T-junctions (top bar) ─────────────────────
        0x252C => ls(L0, Light, Light, Light), // ┬
        0x252D => ls(L0, Light, Light, Heavy), // ┭
        0x252E => ls(L0, Heavy, Light, Light), // ┮
        0x252F => ls(L0, Heavy, Light, Heavy), // ┯
        0x2530 => ls(L0, Light, Heavy, Light), // ┰
        0x2531 => ls(L0, Light, Heavy, Heavy), // ┱
        0x2532 => ls(L0, Heavy, Heavy, Light), // ┲
        0x2533 => ls(L0, Heavy, Heavy, Heavy), // ┳

        // ── Horizontal T-junctions (bottom bar) ──────────────────
        0x2534 => ls(Light, Light, L0, Light), // ┴
        0x2535 => ls(Light, Light, L0, Heavy), // ┵
        0x2536 => ls(Light, Heavy, L0, Light), // ┶
        0x2537 => ls(Light, Heavy, L0, Heavy), // ┷
        0x2538 => ls(Heavy, Light, L0, Light), // ┸
        0x2539 => ls(Heavy, Light, L0, Heavy), // ┹
        0x253A => ls(Heavy, Heavy, L0, Light), // ┺
        0x253B => ls(Heavy, Heavy, L0, Heavy), // ┻

        // ── Cross junctions ──────────────────────────────────────
        0x253C => ls(Light, Light, Light, Light), // ┼
        0x253D => ls(Light, Light, Light, Heavy), // ┽
        0x253E => ls(Light, Heavy, Light, Light), // ┾
        0x253F => ls(Light, Heavy, Light, Heavy), // ┿
        0x2540 => ls(Heavy, Light, Light, Light), // ╀
        0x2541 => ls(Light, Light, Heavy, Light), // ╁
        0x2542 => ls(Heavy, Light, Heavy, Light), // ╂
        0x2543 => ls(Heavy, Light, Light, Heavy), // ╃
        0x2544 => ls(Heavy, Heavy, Light, Light), // ╄
        0x2545 => ls(Light, Light, Heavy, Heavy), // ╅
        0x2546 => ls(Light, Heavy, Heavy, Light), // ╆
        0x2547 => ls(Heavy, Heavy, Light, Heavy), // ╇
        0x2548 => ls(Light, Heavy, Heavy, Heavy), // ╈
        0x2549 => ls(Heavy, Light, Heavy, Heavy), // ╉
        0x254A => ls(Heavy, Heavy, Heavy, Light), // ╊
        0x254B => ls(Heavy, Heavy, Heavy, Heavy), // ╋

        // ── Double-line family ───────────────────────────────────
        0x2550 => ls(L0, Double, L0, Double),       // ═
        0x2551 => ls(Double, L0, Double, L0),       // ║
        0x2552 => ls(L0, Double, Light, L0),        // ╒
        0x2553 => ls(L0, Light, Double, L0),        // ╓
        0x2554 => ls(L0, Double, Double, L0),       // ╔
        0x2555 => ls(L0, L0, Light, Double),        // ╕
        0x2556 => ls(L0, L0, Double, Light),        // ╖
        0x2557 => ls(L0, L0, Double, Double),       // ╗
        0x2558 => ls(Light, Double, L0, L0),        // ╘
        0x2559 => ls(Double, Light, L0, L0),        // ╙
        0x255A => ls(Double, Double, L0, L0),       // ╚
        0x255B => ls(Light, L0, L0, Double),        // ╛
        0x255C => ls(Double, L0, L0, Light),        // ╜
        0x255D => ls(Double, L0, L0, Double),       // ╝
        0x255E => ls(Light, Double, Light, L0),     // ╞
        0x255F => ls(Double, Light, Double, L0),    // ╟
        0x2560 => ls(Double, Double, Double, L0),   // ╠
        0x2561 => ls(Light, L0, Light, Double),     // ╡
        0x2562 => ls(Double, L0, Double, Light),    // ╢
        0x2563 => ls(Double, L0, Double, Double),   // ╣
        0x2564 => ls(L0, Double, Light, Double),    // ╤
        0x2565 => ls(L0, Light, Double, Light),     // ╥
        0x2566 => ls(L0, Double, Double, Double),   // ╦
        0x2567 => ls(Light, Double, L0, Double),    // ╧
        0x2568 => ls(Double, Light, L0, Light),     // ╨
        0x2569 => ls(Double, Double, L0, Double),   // ╩
        0x256A => ls(Light, Double, Light, Double), // ╪
        0x256B => ls(Double, Light, Double, Light), // ╫
        0x256C => ls(Double, Double, Double, Double), // ╬
        _ => return None,
    };
    Some(s)
}

const fn curved_corner_spec(cp: u32) -> Option<LineSpec> {
    let s = match cp {
        0x256D => ls(L0, Light, Light, L0), // ╭
        0x256E => ls(L0, L0, Light, Light), // ╮
        0x256F => ls(Light, L0, L0, Light), // ╯
        0x2570 => ls(Light, Light, L0, L0), // ╰
        _ => return None,
    };
    Some(s)
}

// ─── Half lines + transitions (U+2574..U+257F) ───────────────────────

fn draw_half_or_transition(
    cp: u32,
    m: &Metrics,
    x: f32,
    y: f32,
    color: [f32; 4],
    out: &mut Vec<Rect>,
) -> Option<()> {
    let half_w = (m.cell_w * 0.5).round();
    let half_h = (m.cell_h * 0.5).round();
    match cp {
        0x2574 => {
            // ╴ left half light
            let t = m.light;
            let cy = ((m.cell_h - t) * 0.5).round();
            push(out, x, y + cy, half_w, t, color);
        }
        0x2575 => {
            // ╵ up half light
            let t = m.light;
            let cx = ((m.cell_w - t) * 0.5).round();
            push(out, x + cx, y, t, half_h, color);
        }
        0x2576 => {
            // ╶ right half light
            let t = m.light;
            let cy = ((m.cell_h - t) * 0.5).round();
            push(out, x + half_w, y + cy, m.cell_w - half_w, t, color);
        }
        0x2577 => {
            // ╷ down half light
            let t = m.light;
            let cx = ((m.cell_w - t) * 0.5).round();
            push(out, x + cx, y + half_h, t, m.cell_h - half_h, color);
        }
        0x2578 => {
            // ╸ left half heavy
            let t = m.heavy;
            let cy = ((m.cell_h - t) * 0.5).round();
            push(out, x, y + cy, half_w, t, color);
        }
        0x2579 => {
            // ╹ up half heavy
            let t = m.heavy;
            let cx = ((m.cell_w - t) * 0.5).round();
            push(out, x + cx, y, t, half_h, color);
        }
        0x257A => {
            // ╺ right half heavy
            let t = m.heavy;
            let cy = ((m.cell_h - t) * 0.5).round();
            push(out, x + half_w, y + cy, m.cell_w - half_w, t, color);
        }
        0x257B => {
            // ╻ down half heavy
            let t = m.heavy;
            let cx = ((m.cell_w - t) * 0.5).round();
            push(out, x + cx, y + half_h, t, m.cell_h - half_h, color);
        }
        // Mixed-weight transitions: half of each side gets a different
        // weight. The cell looks like two half-lines glued at the centre.
        0x257C => {
            // ╼ light left, heavy right
            let tl = m.light;
            let th = m.heavy;
            let cyl = ((m.cell_h - tl) * 0.5).round();
            let cyh = ((m.cell_h - th) * 0.5).round();
            push(out, x, y + cyl, half_w, tl, color);
            push(out, x + half_w, y + cyh, m.cell_w - half_w, th, color);
        }
        0x257D => {
            // ╽ light up, heavy down
            let tl = m.light;
            let th = m.heavy;
            let cxl = ((m.cell_w - tl) * 0.5).round();
            let cxh = ((m.cell_w - th) * 0.5).round();
            push(out, x + cxl, y, tl, half_h, color);
            push(out, x + cxh, y + half_h, th, m.cell_h - half_h, color);
        }
        0x257E => {
            // ╾ heavy left, light right
            let tl = m.light;
            let th = m.heavy;
            let cyh = ((m.cell_h - th) * 0.5).round();
            let cyl = ((m.cell_h - tl) * 0.5).round();
            push(out, x, y + cyh, half_w, th, color);
            push(out, x + half_w, y + cyl, m.cell_w - half_w, tl, color);
        }
        0x257F => {
            // ╿ heavy up, light down
            let tl = m.light;
            let th = m.heavy;
            let cxh = ((m.cell_w - th) * 0.5).round();
            let cxl = ((m.cell_w - tl) * 0.5).round();
            push(out, x + cxh, y, th, half_h, color);
            push(out, x + cxl, y + half_h, tl, m.cell_h - half_h, color);
        }
        _ => return None,
    }
    Some(())
}

// ─── Dashed / dotted (U+2504..U+250B, U+254C..U+254F) ────────────────

fn draw_dashed(
    cp: u32,
    m: &Metrics,
    x: f32,
    y: f32,
    color: [f32; 4],
    out: &mut Vec<Rect>,
) -> Option<()> {
    let (segments, axis_horizontal, thickness) = match cp {
        0x2504 => (3, true, m.light),  // ┄ light triple-dash horizontal
        0x2505 => (3, true, m.heavy),  // ┅ heavy triple-dash horizontal
        0x2506 => (3, false, m.light), // ┆ light triple-dash vertical
        0x2507 => (3, false, m.heavy), // ┇ heavy triple-dash vertical
        0x2508 => (4, true, m.light),  // ┈ light quadruple-dash horizontal
        0x2509 => (4, true, m.heavy),  // ┉ heavy quadruple-dash horizontal
        0x250A => (4, false, m.light), // ┊ light quadruple-dash vertical
        0x250B => (4, false, m.heavy), // ┋ heavy quadruple-dash vertical
        0x254C => (2, true, m.light),  // ╌
        0x254D => (2, true, m.heavy),  // ╍
        0x254E => (2, false, m.light), // ╎
        0x254F => (2, false, m.heavy), // ╏
        _ => return None,
    };
    let total = if axis_horizontal { m.cell_w } else { m.cell_h };
    let dash_len = (total / (segments as f32 * 2.0 - 1.0)).max(1.0);
    if axis_horizontal {
        let cy = ((m.cell_h - thickness) * 0.5).round();
        for i in 0..segments {
            let x0 = (i as f32 * (dash_len * 2.0)).round();
            push(out, x + x0, y + cy, dash_len, thickness, color);
        }
    } else {
        let cx = ((m.cell_w - thickness) * 0.5).round();
        for i in 0..segments {
            let y0 = (i as f32 * (dash_len * 2.0)).round();
            push(out, x + cx, y + y0, thickness, dash_len, color);
        }
    }
    Some(())
}

// ─── Block elements (U+2580..U+259F) ────────────────────────────────

fn draw_block_element(
    cp: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    out: &mut Vec<Rect>,
) {
    // For shading, the user-facing colour stays the same but alpha is
    // attenuated so the cell blends with whatever background is behind.
    let with_alpha = |a: f32| [color[0], color[1], color[2], color[3] * a];

    match cp {
        // Lower N/8 blocks: ▁..▇ (1/8 .. 7/8 from the bottom)
        0x2581..=0x2587 => {
            let n = (cp - 0x2580) as f32; // 1..=7
            let bh = (h * n / 8.0).round();
            push(out, x, y + (h - bh), w, bh, color);
        }
        // ▀ upper half block
        0x2580 => {
            let bh = (h * 0.5).round();
            push(out, x, y, w, bh, color);
        }
        // █ full block
        0x2588 => {
            push(out, x, y, w, h, color);
        }
        // Left N/8 blocks: ▉..▏ (7/8 .. 1/8 from the left)
        0x2589..=0x258F => {
            let n = (8 - (cp - 0x2588)) as f32; // 7..=1
            let bw = (w * n / 8.0).round();
            push(out, x, y, bw, h, color);
        }
        // ▐ right half block
        0x2590 => {
            let bw = (w * 0.5).round();
            push(out, x + (w - bw), y, bw, h, color);
        }
        // ░ light shade (25%)
        0x2591 => {
            push(out, x, y, w, h, with_alpha(0.25));
        }
        // ▒ medium shade (50%)
        0x2592 => {
            push(out, x, y, w, h, with_alpha(0.5));
        }
        // ▓ dark shade (75%)
        0x2593 => {
            push(out, x, y, w, h, with_alpha(0.75));
        }
        // ▔ upper one-eighth block
        0x2594 => {
            let bh = (h / 8.0).round().max(1.0);
            push(out, x, y, w, bh, color);
        }
        // ▕ right one-eighth block
        0x2595 => {
            let bw = (w / 8.0).round().max(1.0);
            push(out, x + (w - bw), y, bw, h, color);
        }
        // Quadrant blocks ▖▗▘▙▚▛▜▝▞▟ (10 chars)
        0x2596..=0x259F => {
            let half_w = (w * 0.5).round();
            let half_h = (h * 0.5).round();
            let tl = (x, y, half_w, half_h);
            let tr = (x + half_w, y, w - half_w, half_h);
            let bl = (x, y + half_h, half_w, h - half_h);
            let br = (x + half_w, y + half_h, w - half_w, h - half_h);
            // Bit pattern by codepoint (TL, TR, BL, BR — 1 = filled).
            // Reference: Unicode 16.0 charts.
            let mask = match cp {
                0x2596 => 0b0010, //  ▖ BL
                0x2597 => 0b0001, //  ▗ BR
                0x2598 => 0b1000, //  ▘ TL
                0x2599 => 0b1011, //  ▙ TL,BL,BR
                0x259A => 0b1001, //  ▚ TL,BR
                0x259B => 0b1110, //  ▛ TL,TR,BL
                0x259C => 0b1101, //  ▜ TL,TR,BR
                0x259D => 0b0100, //  ▝ TR
                0x259E => 0b0110, //  ▞ TR,BL
                0x259F => 0b0111, //  ▟ TR,BL,BR
                _ => 0,
            };
            let q = [
                (mask & 0b1000 != 0, tl),
                (mask & 0b0100 != 0, tr),
                (mask & 0b0010 != 0, bl),
                (mask & 0b0001 != 0, br),
            ];
            for (on, (rx, ry, rw, rh)) in q {
                if on {
                    push(out, rx, ry, rw, rh, color);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(ch: char) -> usize {
        let mut v = Vec::new();
        emit(ch, 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v);
        v.len()
    }

    fn x_span(rs: &[Rect]) -> (f32, f32) {
        rs.iter()
            .filter(|r| r.h > 0.0)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), r| {
                (lo.min(r.x), hi.max(r.x + r.w))
            })
    }

    #[test]
    fn horizontal_line_covers_full_cell_width() {
        let mut v = Vec::new();
        let handled = emit('\u{2500}', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v);
        assert!(handled);
        let (lo, hi) = x_span(&v);
        assert_eq!(lo, 0.0);
        assert_eq!(hi, 16.0, "── must reach exactly cell_w so neighbours join");
    }

    #[test]
    fn adjacent_horizontals_touch_with_no_gap_or_overlap() {
        // Two cells side by side. The right-arm of cell-0 and the left-arm
        // of cell-1 must meet flush at x=16: no 1-px gap, no 2x overlap.
        let mut v0 = Vec::new();
        emit('\u{2500}', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v0);
        let mut v1 = Vec::new();
        emit('\u{2500}', 16.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v1);
        // The cell-0 right-most edge must equal cell-1 left-most edge.
        let r0_end = v0
            .iter()
            .map(|r| r.x + r.w)
            .fold(f32::NEG_INFINITY, f32::max);
        let r1_start = v1.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        assert_eq!(
            r0_end, r1_start,
            "── joints must be flush, no gap and no overlap"
        );
    }

    #[test]
    fn cross_emits_full_horizontal_and_vertical() {
        // ┼ light cross: 4 arms (left/right/up/down) → 4 rects. The arms
        // share the centre square, which is fine — same colour, alpha
        // stays at 1.0.
        let mut v = Vec::new();
        assert!(emit('\u{253C}', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v));
        assert!(v.len() >= 2, "cross needs both axes drawn");
        let (lo_x, hi_x) = x_span(&v);
        assert_eq!(lo_x, 0.0);
        assert_eq!(hi_x, 16.0);
    }

    #[test]
    fn full_block_fills_cell() {
        let mut v = Vec::new();
        assert!(emit('\u{2588}', 5.0, 7.0, 16.0, 32.0, [1.0; 4], &mut v));
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].x, v[0].y, v[0].w, v[0].h), (5.0, 7.0, 16.0, 32.0));
    }

    #[test]
    fn quadrant_top_right_only_fills_top_right() {
        let mut v = Vec::new();
        assert!(emit('\u{259D}', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v));
        assert_eq!(v.len(), 1);
        // Top-right starts at half_w, top, has the remaining width.
        let half_w = 8.0_f32;
        let half_h = 16.0_f32;
        assert_eq!(v[0].x, half_w);
        assert_eq!(v[0].y, 0.0);
        assert_eq!(v[0].w, 16.0 - half_w);
        assert_eq!(v[0].h, half_h);
    }

    #[test]
    fn shaded_blocks_attenuate_alpha() {
        let mut v = Vec::new();
        emit('\u{2592}', 0.0, 0.0, 16.0, 32.0, [1.0, 1.0, 1.0, 1.0], &mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].color[3], 0.5, "▒ should be ~50% alpha");
    }

    #[test]
    fn diagonals_fall_through() {
        let mut v = Vec::new();
        // ╱ U+2571 is intentionally NOT covered — caller falls back to font.
        let handled = emit('\u{2571}', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v);
        assert!(!handled);
        assert!(v.is_empty());
    }

    #[test]
    fn out_of_range_returns_false() {
        let mut v = Vec::new();
        assert!(!emit('A', 0.0, 0.0, 16.0, 32.0, [1.0; 4], &mut v));
        assert!(v.is_empty());
    }
}
