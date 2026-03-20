use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct DamageAccumulator {
    pub(crate) full: bool,
    /// Indexed by line. Some((left, right)) = dirty range for that line.
    pub(crate) line_damage: HashMap<u16, (u16, u16)>,
    /// Whether the cursor has moved since last send.
    pub(crate) cursor_dirty: bool,
}

impl DamageAccumulator {

    pub(crate) fn mark_full(&mut self) {
        self.full = true;
        self.line_damage.clear();
    }

    /// Merge damage metadata (line, left, right) tuples into the accumulator.
    pub(crate) fn merge_ranges(&mut self, ranges: &[(u16, u16, u16)]) {
        if self.full {
            return; // already marked for full sync
        }
        for &(line, left, right) in ranges {
            let entry = self.line_damage.entry(line).or_insert((u16::MAX, 0));
            entry.0 = entry.0.min(left);
            entry.1 = entry.1.max(right);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.full && self.line_damage.is_empty() && !self.cursor_dirty
    }

    pub(crate) fn take(&mut self) -> DamageAccumulator {
        std::mem::take(self)
    }
}
