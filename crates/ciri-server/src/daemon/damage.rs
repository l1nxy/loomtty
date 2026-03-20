use ciri_protocol::message::DamageRegion;
use std::collections::HashMap;

pub(crate) struct DamageAccumulator {
    pub(crate) full: bool,
    /// Indexed by line. Some((left, right)) = dirty range for that line.
    pub(crate) line_damage: HashMap<u16, (u16, u16)>,
    /// Whether the cursor has moved since last send.
    pub(crate) cursor_dirty: bool,
}

impl DamageAccumulator {
    pub(crate) fn new() -> Self {
        DamageAccumulator {
            full: false,
            line_damage: HashMap::new(),
            cursor_dirty: false,
        }
    }

    pub(crate) fn mark_full(&mut self) {
        self.full = true;
        self.line_damage.clear();
    }

    pub(crate) fn merge_regions(&mut self, regions: &[DamageRegion]) {
        if self.full {
            return; // already marked for full sync
        }
        for region in regions {
            let entry = self.line_damage.entry(region.line).or_insert((u16::MAX, 0));
            entry.0 = entry.0.min(region.left);
            entry.1 = entry.1.max(region.right);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.full && self.line_damage.is_empty() && !self.cursor_dirty
    }

    pub(crate) fn take(&mut self) -> DamageAccumulator {
        std::mem::replace(self, DamageAccumulator::new())
    }
}
