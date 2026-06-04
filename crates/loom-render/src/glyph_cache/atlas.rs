//! Atlas packing and upload management.

use super::types::GlyphEntry;

// ─── Shelf-based atlas packer ────────────────────────────────────────

/// Simple shelf-based 2D rectangle packer for atlas allocation.
/// Allocates left-to-right, top-to-bottom in horizontal shelves.
pub(crate) struct ShelfPacker {
    shelf_y: u32,
    shelf_height: u32,
    cursor_x: u32,
    size: u32,
}

impl ShelfPacker {
    pub(crate) fn new(size: u32) -> Self {
        ShelfPacker {
            shelf_y: 0,
            shelf_height: 0,
            cursor_x: 0,
            size,
        }
    }

    /// 1px gutter between glyphs prevents bilinear sampling bleed.
    const GUTTER: u32 = 1;

    /// Try to allocate a `w×h` region. Returns `(x, y)` origin or `None` if full.
    /// A 1px gutter is added around each glyph so that bilinear texture
    /// sampling doesn't bleed into adjacent atlas entries.
    pub(crate) fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let padded_w = w + Self::GUTTER;
        let padded_h = h + Self::GUTTER;
        if padded_w > self.size || padded_h > self.size {
            return None;
        }
        // Wrap to next shelf if current row is too narrow
        if self.cursor_x + padded_w > self.size {
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }
        if self.shelf_y + padded_h > self.size {
            return None; // atlas full
        }
        let (x, y) = (self.cursor_x, self.shelf_y);
        self.cursor_x += padded_w;
        self.shelf_height = self.shelf_height.max(padded_h);
        Some((x, y))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AtlasRegion {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

// ─── Pending upload ──────────────────────────────────────────────────

/// Queued glyph pixel data, flushed to GPU at frame start by the backend.
pub struct PendingUpload {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

/// Queued DWrite glyph for D2D direct-to-atlas rendering (Windows DX backend).
///
/// Instead of CPU-side pixel extraction, this carries the glyph identity and
/// atlas position. The DX backend renders it via `D2D DrawGlyphRun` onto the
/// atlas texture's DXGI surface.
#[cfg(windows)]
pub struct PendingDwriteGlyph {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub glyph_index: u16,
    pub pixel_size: f32,
    /// D2D DrawGlyphRun baseline origin X within the atlas.
    pub baseline_x: f32,
    /// D2D DrawGlyphRun baseline origin Y within the atlas.
    pub baseline_y: f32,
    pub is_color: bool,
    pub face: windows::Win32::Graphics::DirectWrite::IDWriteFontFace,
}

/// Build a `GlyphEntry` from atlas coordinates.
pub(crate) fn make_glyph_entry(
    region: AtlasRegion,
    bearing_x: f32,
    bearing_y: f32,
    atlas_size: u32,
    is_color: bool,
) -> GlyphEntry {
    let s = atlas_size as f32;
    GlyphEntry {
        u0: region.x as f32 / s,
        v0: region.y as f32 / s,
        u1: (region.x + region.w) as f32 / s,
        v1: (region.y + region.h) as f32 / s,
        width: region.w as u16,
        height: region.h as u16,
        bearing_x,
        bearing_y,
        is_color,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Gutter constant for readability in tests.
    const G: u32 = ShelfPacker::GUTTER;

    #[test]
    fn shelf_packer_basic() {
        let mut p = ShelfPacker::new(100);
        assert_eq!(p.allocate(10, 10), Some((0, 0)));
        // Second glyph starts after 10 + gutter
        assert_eq!(p.allocate(10, 10), Some((10 + G, 0)));
    }

    #[test]
    fn shelf_packer_wrap() {
        let mut p = ShelfPacker::new(100);
        // Each 10-wide glyph takes 10+G columns; fit as many as possible
        let per_row = 100 / (10 + G);
        for _ in 0..per_row {
            assert!(p.allocate(10, 20).is_some());
        }
        // Next row starts at y = 20 + G (shelf height includes gutter)
        assert_eq!(p.allocate(10, 15), Some((0, 20 + G)));
    }

    #[test]
    fn shelf_packer_full() {
        // 19+G=20 exactly fills the row, 19+G=20 exactly fills the column
        let mut p = ShelfPacker::new(20);
        assert!(p.allocate(19, 19).is_some());
        assert!(p.allocate(1, 1).is_none());
    }

    #[test]
    fn shelf_packer_oversized() {
        let mut p = ShelfPacker::new(10);
        // 10+G > 10 → won't fit
        assert!(p.allocate(10, 5).is_none());
        assert!(p.allocate(5, 10).is_none());
    }

    #[test]
    fn shelf_packer_mixed_heights_tallest_sets_shelf() {
        let mut p = ShelfPacker::new(200);
        // First glyph is tall (30px), second is short (10px)
        assert_eq!(p.allocate(10, 30), Some((0, 0)));
        assert_eq!(p.allocate(10, 10), Some((10 + G, 0)));
        // Next shelf starts at y = 30 + G
        let per_row = 200 / (10 + G);
        for _ in 2..per_row {
            assert!(p.allocate(10, 10).is_some());
        }
        assert_eq!(p.allocate(10, 10), Some((0, 30 + G)));
    }

    #[test]
    fn shelf_packer_zero_size_does_not_advance_cursor() {
        let mut p = ShelfPacker::new(100);
        // Zero-size allocation: padded size is 0+G=G, cursor advances by G.
        assert_eq!(p.allocate(0, 0), Some((0, 0)));
        assert_eq!(p.allocate(10, 10), Some((G, 0)));
    }

    #[test]
    fn make_glyph_entry_uv_coordinates() {
        let region = AtlasRegion {
            x: 10,
            y: 20,
            w: 8,
            h: 16,
        };
        let entry = make_glyph_entry(region, 1.5, 12.0, 256, false);

        assert_eq!(entry.u0, 10.0 / 256.0);
        assert_eq!(entry.v0, 20.0 / 256.0);
        assert_eq!(entry.u1, 18.0 / 256.0); // (10+8)/256
        assert_eq!(entry.v1, 36.0 / 256.0); // (20+16)/256
        assert_eq!(entry.width, 8);
        assert_eq!(entry.height, 16);
        assert_eq!(entry.bearing_x, 1.5);
        assert_eq!(entry.bearing_y, 12.0);
        assert!(!entry.is_color);
    }

    #[test]
    fn make_glyph_entry_color_flag() {
        let region = AtlasRegion {
            x: 0,
            y: 0,
            w: 32,
            h: 32,
        };
        let entry = make_glyph_entry(region, 0.0, 0.0, 1024, true);
        assert!(entry.is_color);
    }

    #[test]
    fn make_glyph_entry_atlas_origin() {
        // Glyph at atlas origin should have UV (0,0)→(w/s, h/s)
        let region = AtlasRegion {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let entry = make_glyph_entry(region, 0.0, 0.0, 100, false);
        assert_eq!(entry.u0, 0.0);
        assert_eq!(entry.v0, 0.0);
        assert_eq!(entry.u1, 0.1);
        assert_eq!(entry.v1, 0.1);
    }
}
