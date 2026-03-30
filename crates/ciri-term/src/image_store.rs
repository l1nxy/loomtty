//! Image placement storage for active and pending images.

use crate::pane::ImagePlacement;

const MAX_ACTIVE_IMAGES: usize = 64;

/// Manages active (persistent) and pending (broadcast-once) image placements.
pub(crate) struct ImageStore {
    /// Persistent images — survives drain, used for reconnecting clients.
    active: Vec<ImagePlacement>,
    /// Newly added since last drain — broadcast to clients then cleared.
    pending: Vec<ImagePlacement>,
    /// Whether a delete command was observed since the last drain.
    deleted: bool,
}

impl ImageStore {
    pub fn new() -> Self {
        Self {
            active: Vec::new(),
            pending: Vec::new(),
            deleted: false,
        }
    }

    /// Add newly scanned placements to both active and pending sets.
    pub fn add_placements(&mut self, placements: Vec<ImagePlacement>) {
        self.pending.extend(placements);
    }

    /// Handle a kitty delete command: clear both active and pending.
    pub fn clear_on_delete(&mut self) {
        self.active.clear();
        self.pending.clear();
        self.deleted = true;
    }

    /// Get a mutable reference to active images (parsers write directly).
    pub fn active_mut(&mut self) -> &mut Vec<ImagePlacement> {
        &mut self.active
    }

    /// Get active images (for reconnection).
    pub fn active(&self) -> &[ImagePlacement] {
        &self.active
    }

    /// Drain pending images (returns them and clears the pending buffer).
    pub fn drain_pending(&mut self) -> Vec<ImagePlacement> {
        std::mem::take(&mut self.pending)
    }

    /// Drain and reset the delete marker.
    pub fn drain_deleted(&mut self) -> bool {
        std::mem::take(&mut self.deleted)
    }

    /// Cap active images to prevent unbounded growth.
    pub fn cap_active(&mut self) {
        if self.active.len() > MAX_ACTIVE_IMAGES {
            let excess = self.active.len() - MAX_ACTIVE_IMAGES;
            self.active.drain(..excess);
        }
    }
}
