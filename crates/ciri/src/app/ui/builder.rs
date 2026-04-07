//! Lightweight immediate-mode UI layout builder.
//!
//! Sits on top of the existing glyph atlas + rect rendering pipeline.
//! Tracks a cursor within nested horizontal/vertical regions and emits
//! `Rect` and `GlyphInstance` as widgets are drawn. Hit rects are recorded
//! as a side-effect of layout for use in click/hover dispatch.

use ciri_render::rect::Rect;
use unicode_width::UnicodeWidthStr;

use super::types::{UiContext, UiScene};
use crate::app::status_bar::{TextEmitParams, emit_status_text};

// ─── Hit record ──────────────────────────────────────────────────────

/// A named pixel-aligned clickable region produced during layout.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitRect {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HitRect {
    #[inline]
    pub fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx < self.x + self.w && my >= self.y && my < self.y + self.h
    }
}

// ─── Response ────────────────────────────────────────────────────────

/// Returned by interactive widgets. `clicked` and `hovered` are computed
/// eagerly from the mouse position stored in `UiBuilder`.
pub(crate) struct Response {
    pub rect: HitRect,
    pub hovered: bool,
    pub clicked: bool,
}

// ─── Region ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

/// Layout region: an allocation box with a cursor that advances along one axis.
#[derive(Clone, Copy)]
struct Region {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    cursor_x: f32,
    cursor_y: f32,
    axis: Axis,
    gap: f32,
}

impl Region {
    fn new(x: f32, y: f32, w: f32, h: f32, gap: f32, axis: Axis) -> Self {
        Self { x, y, w, h, cursor_x: x, cursor_y: y, axis, gap }
    }

    /// Allocate `item_w × item_h` at the current cursor.
    /// Returns the top-left of the allocated rect, then advances the cursor.
    fn allocate(&mut self, item_w: f32, item_h: f32) -> Option<(f32, f32)> {
        match self.axis {
            Axis::Horizontal => {
                let remaining = (self.x + self.w) - self.cursor_x;
                if item_w > remaining + 0.5 {
                    return None;
                }
                let rx = self.cursor_x;
                self.cursor_x += item_w + self.gap;
                Some((rx, self.cursor_y))
            }
            Axis::Vertical => {
                let remaining = (self.y + self.h) - self.cursor_y;
                if item_h > remaining + 0.5 {
                    return None;
                }
                let ry = self.cursor_y;
                self.cursor_y += item_h + self.gap;
                Some((self.cursor_x, ry))
            }
        }
    }

    fn remaining(&self) -> f32 {
        match self.axis {
            Axis::Horizontal => ((self.x + self.w) - self.cursor_x).max(0.0),
            Axis::Vertical => ((self.y + self.h) - self.cursor_y).max(0.0),
        }
    }

    fn consumed(&self) -> f32 {
        match self.axis {
            Axis::Horizontal => self.cursor_x - self.x,
            Axis::Vertical => self.cursor_y - self.y,
        }
    }
}

// ─── UiBuilder ───────────────────────────────────────────────────────

pub(crate) struct UiBuilder<'a, 'b> {
    cx: &'a UiContext<'a>,
    scene: &'a mut UiScene<'b>,
    region: Region,
    hits: Vec<HitRect>,
    mouse_x: f32,
    mouse_y: f32,
    mouse_pressed: bool,
}

impl<'a, 'b> UiBuilder<'a, 'b> {
    pub fn new_horizontal(
        x: f32, y: f32, w: f32, h: f32, gap: f32,
        mouse_x: f32, mouse_y: f32, mouse_pressed: bool,
        cx: &'a UiContext<'a>,
        scene: &'a mut UiScene<'b>,
    ) -> Self {
        Self {
            cx, scene,
            region: Region::new(x, y, w, h, gap, Axis::Horizontal),
            hits: Vec::new(),
            mouse_x, mouse_y, mouse_pressed,
        }
    }

    pub fn new_vertical(
        x: f32, y: f32, w: f32, h: f32, gap: f32,
        mouse_x: f32, mouse_y: f32, mouse_pressed: bool,
        cx: &'a UiContext<'a>,
        scene: &'a mut UiScene<'b>,
    ) -> Self {
        Self {
            cx, scene,
            region: Region::new(x, y, w, h, gap, Axis::Vertical),
            hits: Vec::new(),
            mouse_x, mouse_y, mouse_pressed,
        }
    }

    // ─── Accessors ───────────────────────────────────────────────────

    pub fn cx(&self) -> &UiContext<'a> {
        self.cx
    }

    pub fn remaining(&self) -> f32 {
        self.region.remaining()
    }

    pub fn cursor_pos(&self) -> (f32, f32) {
        (self.region.cursor_x, self.region.cursor_y)
    }

    pub fn region_rect(&self) -> (f32, f32, f32, f32) {
        (self.region.x, self.region.y, self.region.w, self.region.h)
    }

    // ─── Sub-layouts ─────────────────────────────────────────────────

    /// Allocate a horizontal sub-region. `w = None` fills remaining space.
    pub fn horizontal(
        &mut self, w: Option<f32>, h: f32, gap: f32,
        f: impl FnOnce(&mut UiBuilder<'_, 'b>),
    ) -> f32 {
        let avail_w = w.unwrap_or(self.region.remaining());
        let Some((rx, ry)) = self.region.allocate(avail_w, h) else {
            return 0.0;
        };
        let mut sub = UiBuilder {
            cx: self.cx,
            scene: self.scene,
            region: Region::new(rx, ry, avail_w, h, gap, Axis::Horizontal),
            hits: Vec::new(),
            mouse_x: self.mouse_x,
            mouse_y: self.mouse_y,
            mouse_pressed: self.mouse_pressed,
        };
        f(&mut sub);
        let consumed = sub.region.consumed();
        self.hits.extend(sub.hits);
        consumed
    }

    /// Allocate a vertical sub-region. `h = None` fills remaining space.
    pub fn vertical(
        &mut self, w: f32, h: Option<f32>, gap: f32,
        f: impl FnOnce(&mut UiBuilder<'_, 'b>),
    ) -> f32 {
        let avail_h = h.unwrap_or(self.region.remaining());
        let Some((rx, ry)) = self.region.allocate(w, avail_h) else {
            return 0.0;
        };
        let mut sub = UiBuilder {
            cx: self.cx,
            scene: self.scene,
            region: Region::new(rx, ry, w, avail_h, gap, Axis::Vertical),
            hits: Vec::new(),
            mouse_x: self.mouse_x,
            mouse_y: self.mouse_y,
            mouse_pressed: self.mouse_pressed,
        };
        f(&mut sub);
        let consumed = sub.region.consumed();
        self.hits.extend(sub.hits);
        consumed
    }

    // ─── Hit extraction ──────────────────────────────────────────────

    /// Consume the builder and return all recorded hit rects.
    pub fn into_hits(self) -> Vec<HitRect> {
        self.hits
    }

    // ─── Measurement ─────────────────────────────────────────────────

    /// Pixel width of `text` in the monospace grid.
    pub fn text_width(&self, text: &str) -> f32 {
        UnicodeWidthStr::width(text) as f32 * self.cx.cell_w
    }

    /// Height of one text row.
    pub fn row_height(&self) -> f32 {
        self.cx.cell_h
    }

    // ─── Primitives ──────────────────────────────────────────────────

    /// Background rect that participates in layout (advances cursor).
    pub fn bg_rect(&mut self, w: f32, h: f32, color: [f32; 4]) {
        if let Some((rx, ry)) = self.region.allocate(w, h) {
            self.scene.bg_rects.push(Rect { x: rx, y: ry, w, h, color });
        }
    }

    /// Background rect at absolute position (does NOT advance cursor).
    pub fn abs_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.scene.bg_rects.push(Rect { x, y, w, h, color });
    }

    /// Emit text at absolute position (does NOT advance cursor).
    pub fn abs_text(&mut self, text: &str, x: f32, y: f32, color: [f32; 4]) {
        emit_status_text(
            self.scene.atlas, text,
            &TextEmitParams {
                x_start: x, y,
                cell_width: self.cx.cell_w,
                baseline: self.cx.baseline,
                color,
            },
            self.scene.glyphs,
            self.scene.color_glyphs,
        );
    }

    // Internal text emit — delegates to abs_text.
    fn emit_text(&mut self, text: &str, x: f32, y: f32, color: [f32; 4]) {
        self.abs_text(text, x, y, color);
    }

    fn make_response(&self, id: u32, rx: f32, ry: f32, rw: f32, rh: f32) -> Response {
        let hit = HitRect { id, x: rx, y: ry, w: rw, h: rh };
        let hovered = hit.contains(self.mouse_x, self.mouse_y);
        Response { rect: hit, hovered, clicked: hovered && self.mouse_pressed }
    }

    // ─── Widgets ─────────────────────────────────────────────────────

    /// Non-interactive text label.
    pub fn label(&mut self, text: &str, color: [f32; 4]) {
        let tw = self.text_width(text);
        let h = self.cx.cell_h;
        if let Some((rx, ry)) = self.region.allocate(tw, h) {
            self.emit_text(text, rx, ry, color);
        }
    }

    /// Clickable text button with optional hover background.
    pub fn button(&mut self, id: u32, text: &str, color: [f32; 4], hover_bg: [f32; 4]) -> Response {
        let tw = self.text_width(text);
        let h = self.cx.cell_h;
        let Some((rx, ry)) = self.region.allocate(tw, h) else {
            return self.make_response(id, 0.0, 0.0, 0.0, 0.0);
        };
        let resp = self.make_response(id, rx, ry, tw, h);
        if resp.hovered {
            self.scene.bg_rects.push(Rect { x: rx, y: ry, w: tw, h, color: hover_bg });
        }
        self.emit_text(text, rx, ry, color);
        self.hits.push(resp.rect);
        resp
    }

    /// Selectable label with active indicator (underline/overline).
    pub fn selectable_label(
        &mut self, id: u32, text: &str, selected: bool,
        active_color: [f32; 4], inactive_color: [f32; 4],
        indicator_color: [f32; 4], indicator_thickness: f32,
        indicator_top: bool,
    ) -> Response {
        let tw = self.text_width(text);
        let h = self.region.h;
        let Some((rx, ry)) = self.region.allocate(tw, h) else {
            return self.make_response(id, 0.0, 0.0, 0.0, 0.0);
        };
        let resp = self.make_response(id, rx, ry, tw, h);
        if selected {
            let iy = if indicator_top { ry } else { ry + h - indicator_thickness };
            self.scene.bg_rects.push(Rect {
                x: rx, y: iy, w: tw, h: indicator_thickness, color: indicator_color,
            });
        }
        let color = if selected || resp.hovered { active_color } else { inactive_color };
        self.emit_text(text, rx, ry, color);
        self.hits.push(resp.rect);
        resp
    }

    /// Consume all remaining space (flex spacer).
    pub fn spacer(&mut self) {
        let r = self.region.remaining();
        match self.region.axis {
            Axis::Horizontal => self.region.cursor_x += r,
            Axis::Vertical => self.region.cursor_y += r,
        }
    }

    /// Thin vertical separator line (inside horizontal region).
    pub fn separator_v(&mut self, color: [f32; 4], inset: f32) {
        let w = 1.0_f32;
        let h = self.region.h;
        if let Some((rx, ry)) = self.region.allocate(w, h) {
            self.scene.bg_rects.push(Rect {
                x: rx, y: ry + inset, w, h: h - inset * 2.0, color,
            });
        }
    }

    /// Thin horizontal separator line (inside vertical region).
    pub fn separator_h(&mut self, color: [f32; 4], inset: f32) {
        let w = self.region.w;
        let h = 1.0_f32;
        if let Some((_rx, ry)) = self.region.allocate(w, h) {
            self.scene.bg_rects.push(Rect {
                x: self.region.x, y: ry, w, h, color,
            });
        }
    }

    /// Text input field with prefix and cursor.
    pub fn text_input(
        &mut self, id: u32, prefix: &str, value: &str, w: f32,
        fg: [f32; 4], bg: [f32; 4], cursor_color: [f32; 4],
    ) -> Response {
        let h = self.cx.cell_h;
        let Some((rx, ry)) = self.region.allocate(w, h) else {
            return self.make_response(id, 0.0, 0.0, 0.0, 0.0);
        };
        self.scene.bg_rects.push(Rect { x: rx, y: ry, w, h, color: bg });
        let display = format!("{}{}", prefix, value);
        self.emit_text(&display, rx, ry, fg);
        // Blinking cursor after text
        let cursor_x = rx + self.text_width(&display);
        self.scene.bg_rects.push(Rect { x: cursor_x, y: ry, w: 2.0, h, color: cursor_color });
        let resp = self.make_response(id, rx, ry, w, h);
        self.hits.push(resp.rect);
        resp
    }

    /// Scrollable list. Draws `visible_count` rows via `row_fn`, plus scrollbar.
    pub fn scroll_list(
        &mut self, w: f32, row_h: f32,
        visible_count: usize, total_count: usize, scroll_offset: usize,
        accent: [f32; 4], border: [f32; 4],
        mut row_fn: impl FnMut(usize, &mut UiBuilder<'_, 'b>),
    ) {
        let total_h = visible_count as f32 * row_h;
        let Some((rx, ry)) = self.region.allocate(w, total_h) else { return; };

        for vis in 0..visible_count {
            let abs_idx = scroll_offset + vis;
            if abs_idx >= total_count {
                break;
            }
            let row_y = ry + vis as f32 * row_h;
            let mut row_ui = UiBuilder {
                cx: self.cx,
                scene: self.scene,
                region: Region::new(rx, row_y, w, row_h, 0.0, Axis::Horizontal),
                hits: Vec::new(),
                mouse_x: self.mouse_x,
                mouse_y: self.mouse_y,
                mouse_pressed: self.mouse_pressed,
            };
            row_fn(abs_idx, &mut row_ui);
            self.hits.extend(row_ui.hits);
        }

        // Scrollbar
        if total_count > visible_count {
            let track_w = 4.0;
            let track_x = rx + w - 8.0;
            let track_y = ry + 2.0;
            let track_h = (total_h - 4.0).max(0.0);
            self.scene.bg_rects.push(Rect {
                x: track_x, y: track_y, w: track_w, h: track_h,
                color: [border[0], border[1], border[2], 0.20],
            });
            let ratio = visible_count as f32 / total_count as f32;
            let thumb_h = (track_h * ratio).max(row_h * 0.75);
            let scroll_ratio = scroll_offset as f32 / total_count.saturating_sub(visible_count).max(1) as f32;
            let thumb_y = track_y + (track_h - thumb_h).max(0.0) * scroll_ratio;
            self.scene.bg_rects.push(Rect {
                x: track_x, y: thumb_y, w: track_w, h: thumb_h,
                color: [accent[0], accent[1], accent[2], 0.65],
            });
        }
    }

    /// Full-screen dimmed backdrop for modal overlays.
    pub fn modal_backdrop(&mut self, color: [f32; 4]) {
        self.scene.bg_rects.push(Rect {
            x: 0.0, y: 0.0, w: self.cx.viewport_w, h: self.cx.viewport_h, color,
        });
    }

    /// Bordered panel with border expanding OUTWARD from (x,y,w,h).
    /// Total painted area is (x-bw, y-bw, w+2*bw, h+2*bw).
    /// Used by palette (which expects outer expansion).
    pub fn bordered_panel(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        bg: [f32; 4], border: [f32; 4], bw: f32, shadow: bool,
    ) -> (f32, f32, f32, f32) {
        if shadow {
            self.scene.bg_rects.push(Rect {
                x: x + 3.0, y: y + 3.0, w, h, color: [0.0, 0.0, 0.0, 0.4],
            });
        }
        self.scene.bg_rects.push(Rect {
            x: x - bw, y: y - bw, w: w + bw * 2.0, h: h + bw * 2.0, color: border,
        });
        self.scene.bg_rects.push(Rect { x, y, w, h, color: bg });
        (x, y, w, h)
    }

    /// Bordered panel with border drawn INSIDE (x,y,w,h) as 4 edge lines.
    /// Total painted area stays exactly (x, y, w, h).
    /// Used by context_menu, info_box, paste_dialog (which expect inset borders).
    pub fn bordered_panel_inset(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        bg: [f32; 4], border: [f32; 4], bw: f32, shadow: bool,
    ) -> (f32, f32, f32, f32) {
        if shadow {
            self.scene.bg_rects.push(Rect {
                x: x + 3.0, y: y + 3.0, w, h, color: [0.0, 0.0, 0.0, 0.4],
            });
        }
        // Background fill
        self.scene.bg_rects.push(Rect { x, y, w, h, color: bg });
        // 4 inset border edges
        self.scene.bg_rects.push(Rect { x, y, w, h: bw, color: border });           // top
        self.scene.bg_rects.push(Rect { x, y: y + h - bw, w, h: bw, color: border }); // bottom
        self.scene.bg_rects.push(Rect { x, y, w: bw, h, color: border });           // left
        self.scene.bg_rects.push(Rect { x: x + w - bw, y, w: bw, h, color: border }); // right
        (x + bw, y + bw, w - bw * 2.0, h - bw * 2.0)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_horizontal_allocate_advances_cursor() {
        let mut r = Region::new(10.0, 20.0, 100.0, 30.0, 2.0, Axis::Horizontal);
        let pos1 = r.allocate(20.0, 30.0);
        assert_eq!(pos1, Some((10.0, 20.0)));
        assert_eq!(r.cursor_x, 32.0); // 10 + 20 + 2(gap)

        let pos2 = r.allocate(30.0, 30.0);
        assert_eq!(pos2, Some((32.0, 20.0)));
        assert_eq!(r.cursor_x, 64.0); // 32 + 30 + 2
    }

    #[test]
    fn region_vertical_allocate_advances_cursor() {
        let mut r = Region::new(0.0, 0.0, 50.0, 100.0, 4.0, Axis::Vertical);
        let pos1 = r.allocate(50.0, 25.0);
        assert_eq!(pos1, Some((0.0, 0.0)));
        assert_eq!(r.cursor_y, 29.0); // 0 + 25 + 4

        let pos2 = r.allocate(50.0, 25.0);
        assert_eq!(pos2, Some((0.0, 29.0)));
    }

    #[test]
    fn region_allocate_returns_none_when_full() {
        let mut r = Region::new(0.0, 0.0, 50.0, 30.0, 0.0, Axis::Horizontal);
        assert!(r.allocate(50.0, 30.0).is_some());
        assert!(r.allocate(1.0, 30.0).is_none()); // no space left
    }

    #[test]
    fn region_remaining_and_consumed() {
        let mut r = Region::new(0.0, 0.0, 100.0, 50.0, 0.0, Axis::Horizontal);
        assert_eq!(r.remaining(), 100.0);
        assert_eq!(r.consumed(), 0.0);

        r.allocate(40.0, 50.0);
        assert_eq!(r.remaining(), 60.0);
        assert_eq!(r.consumed(), 40.0);
    }

    #[test]
    fn region_gap_is_applied() {
        let mut r = Region::new(0.0, 0.0, 100.0, 20.0, 5.0, Axis::Horizontal);
        r.allocate(10.0, 20.0);
        // cursor should be at 10 + 5(gap) = 15
        assert_eq!(r.cursor_x, 15.0);
        let pos = r.allocate(10.0, 20.0);
        assert_eq!(pos, Some((15.0, 0.0)));
    }

    #[test]
    fn hit_rect_contains() {
        let h = HitRect { id: 0, x: 10.0, y: 20.0, w: 30.0, h: 15.0 };
        assert!(h.contains(10.0, 20.0));   // top-left corner
        assert!(h.contains(25.0, 30.0));   // inside
        assert!(!h.contains(40.0, 20.0));  // right edge (exclusive)
        assert!(!h.contains(9.0, 20.0));   // just outside left
        assert!(!h.contains(10.0, 35.0));  // just outside bottom
    }

    #[test]
    fn hit_rect_zero_size_contains_nothing() {
        let h = HitRect { id: 0, x: 5.0, y: 5.0, w: 0.0, h: 0.0 };
        assert!(!h.contains(5.0, 5.0));
    }
}
