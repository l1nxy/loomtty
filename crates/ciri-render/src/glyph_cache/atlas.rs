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

    /// Try to allocate a `w×h` region. Returns `(x, y)` origin or `None` if full.
    pub(crate) fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.size || h > self.size {
            return None;
        }
        // Wrap to next shelf if current row is too narrow
        if self.cursor_x + w > self.size {
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }
        if self.shelf_y + h > self.size {
            return None; // atlas full
        }
        let (x, y) = (self.cursor_x, self.shelf_y);
        self.cursor_x += w;
        self.shelf_height = self.shelf_height.max(h);
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

    #[test]
    fn shelf_packer_basic() {
        let mut p = ShelfPacker::new(100);
        assert_eq!(p.allocate(10, 10), Some((0, 0)));
        assert_eq!(p.allocate(10, 10), Some((10, 0)));
    }

    #[test]
    fn shelf_packer_wrap() {
        let mut p = ShelfPacker::new(100);
        for _ in 0..10 {
            assert!(p.allocate(10, 20).is_some());
        }
        assert_eq!(p.allocate(10, 15), Some((0, 20)));
    }

    #[test]
    fn shelf_packer_full() {
        let mut p = ShelfPacker::new(20);
        assert!(p.allocate(20, 20).is_some());
        assert!(p.allocate(1, 1).is_none());
    }

    #[test]
    fn shelf_packer_oversized() {
        let mut p = ShelfPacker::new(10);
        assert!(p.allocate(11, 5).is_none());
        assert!(p.allocate(5, 11).is_none());
    }
}
