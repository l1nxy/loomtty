use ciri_protocol::message::ImageDisplayMode;
use std::collections::VecDeque;
use std::sync::Arc;

pub type PaneId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticZone {
    Prompt,
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct ImagePlacement {
    pub id: u64,
    pub row: u16,
    pub col: u16,
    pub width_cells: u16,
    pub height_cells: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub display_mode: ImageDisplayMode,
    pub format: String,
    pub data: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ShellState {
    pub zone: SemanticZone,
    pub last_exit_code: Option<i32>,
    pub prompt_line: Option<i32>,
    pub output_line: Option<i32>,
    pub command_start: Option<std::time::Instant>,
}

/// A semantic prompt boundary captured from OSC 133 (shell integration).
///
/// All line indices are in **absolute-line** space: `scrollback_total +
/// row_at_event_time`. The value stays stable as rows scroll into history,
/// so an old mark still identifies the same content after the user has
/// kept working. Lines that drop out of the scrollback ring buffer become
/// unreachable in the grid but their `PromptMark` remains for as long as
/// the ring caches it — callers should treat `prompt_line` below
/// `scrollback_total - history_size` as evicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptMark {
    /// Absolute line where OSC 133;A (PromptStart) was observed.
    pub prompt_line: u64,
    /// Absolute line where OSC 133;C (CommandOutput) was observed, if any.
    /// `None` when the user submitted an empty command (no C between A and D),
    /// or when D arrived without a preceding C.
    pub output_line: Option<u64>,
    /// Absolute line where OSC 133;D (Done) was observed, if any.
    /// `None` while the command is still running.
    pub done_line: Option<u64>,
    /// Exit code from OSC 133;D parameters, if present.
    pub exit_code: Option<i32>,
    /// Duration between the OSC 133;C and OSC 133;D observations.
    /// Stored as `(secs, nanos)` to keep `Copy` and avoid heap.
    duration_secs: u64,
    duration_nanos: u32,
    duration_set: bool,
}

impl PromptMark {
    pub(crate) fn new(prompt_line: u64) -> Self {
        Self {
            prompt_line,
            output_line: None,
            done_line: None,
            exit_code: None,
            duration_secs: 0,
            duration_nanos: 0,
            duration_set: false,
        }
    }

    pub fn duration(&self) -> Option<std::time::Duration> {
        if self.duration_set {
            Some(std::time::Duration::new(
                self.duration_secs,
                self.duration_nanos,
            ))
        } else {
            None
        }
    }

    pub(crate) fn set_duration(&mut self, d: std::time::Duration) {
        self.duration_secs = d.as_secs();
        self.duration_nanos = d.subsec_nanos();
        self.duration_set = true;
    }
}

/// Bounded ring buffer of [`PromptMark`]s. Oldest is dropped on overflow.
#[derive(Debug, Clone)]
pub struct PromptMarkRing {
    marks: VecDeque<PromptMark>,
    /// Index of the current in-progress mark (between OSC 133;A and OSC 133;D).
    /// `None` when no command is currently being tracked.
    in_progress: Option<usize>,
    capacity: usize,
}

impl PromptMarkRing {
    pub const DEFAULT_CAPACITY: usize = 1024;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            marks: VecDeque::with_capacity(capacity),
            in_progress: None,
            capacity,
        }
    }

    pub fn len(&self) -> usize {
        self.marks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PromptMark> {
        self.marks.iter()
    }

    pub fn as_slice(&self) -> Vec<PromptMark> {
        self.marks.iter().copied().collect()
    }

    /// Start a new prompt cycle at `abs_line`. Any in-progress mark is
    /// finalized as-is (Done was never observed for it — keep what we have).
    pub(crate) fn begin_prompt(&mut self, abs_line: u64) {
        // The previously-in-progress mark stays in `marks` exactly as it was;
        // we just stop tracking it as in-progress so the next D doesn't write
        // through to it.
        self.in_progress = None;
        self.push(PromptMark::new(abs_line));
        self.in_progress = Some(self.marks.len() - 1);
    }

    /// Record OSC 133;C — start of command output.
    pub(crate) fn mark_output(&mut self, abs_line: u64) {
        if let Some(idx) = self.in_progress
            && let Some(m) = self.marks.get_mut(idx)
        {
            m.output_line = Some(abs_line);
        }
    }

    /// Record OSC 133;D — command done.
    pub(crate) fn mark_done(
        &mut self,
        abs_line: u64,
        exit_code: Option<i32>,
        duration: Option<std::time::Duration>,
    ) {
        if let Some(idx) = self.in_progress.take()
            && let Some(m) = self.marks.get_mut(idx)
        {
            m.done_line = Some(abs_line);
            m.exit_code = exit_code;
            if let Some(d) = duration {
                m.set_duration(d);
            }
        }
    }

    /// Drop marks whose `prompt_line` has been evicted from the scrollback
    /// ring buffer. `min_reachable_abs_line` is `scrollback_total - history_cap`
    /// at the caller's side. No-op when nothing is evictable.
    pub(crate) fn prune_below(&mut self, min_reachable_abs_line: u64) {
        while let Some(front) = self.marks.front() {
            if front.prompt_line < min_reachable_abs_line {
                self.marks.pop_front();
                if let Some(idx) = self.in_progress {
                    self.in_progress = idx.checked_sub(1);
                }
            } else {
                break;
            }
        }
    }

    fn push(&mut self, mark: PromptMark) {
        if self.marks.len() == self.capacity {
            self.marks.pop_front();
            if let Some(idx) = self.in_progress {
                self.in_progress = idx.checked_sub(1);
            }
        }
        self.marks.push_back(mark);
    }
}

impl Default for PromptMarkRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn full_cycle_records_three_lines_and_exit_code() {
        let mut ring = PromptMarkRing::new();
        ring.begin_prompt(100);
        ring.mark_output(101);
        ring.mark_done(105, Some(0), Some(Duration::from_millis(42)));

        assert_eq!(ring.len(), 1);
        let m = ring.iter().next().unwrap();
        assert_eq!(m.prompt_line, 100);
        assert_eq!(m.output_line, Some(101));
        assert_eq!(m.done_line, Some(105));
        assert_eq!(m.exit_code, Some(0));
        assert_eq!(m.duration(), Some(Duration::from_millis(42)));
    }

    #[test]
    fn done_without_output_keeps_output_line_none() {
        let mut ring = PromptMarkRing::new();
        ring.begin_prompt(50);
        // No mark_output — user pressed Enter on an empty line.
        ring.mark_done(50, Some(0), None);

        let m = ring.iter().next().unwrap();
        assert_eq!(m.output_line, None);
        assert_eq!(m.done_line, Some(50));
        assert_eq!(m.duration(), None);
    }

    #[test]
    fn second_prompt_starts_new_mark_without_disturbing_previous() {
        let mut ring = PromptMarkRing::new();
        ring.begin_prompt(10);
        ring.mark_done(15, Some(0), None);
        ring.begin_prompt(20);
        ring.mark_done(25, Some(1), None);

        let marks: Vec<_> = ring.iter().copied().collect();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].prompt_line, 10);
        assert_eq!(marks[0].exit_code, Some(0));
        assert_eq!(marks[1].prompt_line, 20);
        assert_eq!(marks[1].exit_code, Some(1));
    }

    #[test]
    fn second_prompt_before_done_finalises_previous_in_place() {
        // Some shells send another OSC 133;A without an intervening D
        // (e.g. Ctrl+C during command input). The previous mark stays
        // as-is (no done_line, no exit_code), and the new prompt starts
        // a fresh in-progress entry.
        let mut ring = PromptMarkRing::new();
        ring.begin_prompt(10);
        ring.mark_output(11);
        // No mark_done.
        ring.begin_prompt(20);
        ring.mark_done(25, Some(130), None);

        let marks: Vec<_> = ring.iter().copied().collect();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].prompt_line, 10);
        assert_eq!(marks[0].output_line, Some(11));
        assert_eq!(marks[0].done_line, None);
        assert_eq!(marks[1].prompt_line, 20);
        assert_eq!(marks[1].done_line, Some(25));
        assert_eq!(marks[1].exit_code, Some(130));
    }

    #[test]
    fn prune_below_drops_old_marks() {
        let mut ring = PromptMarkRing::new();
        for i in 0..5u64 {
            ring.begin_prompt(i * 10);
            ring.mark_done(i * 10 + 1, Some(0), None);
        }
        assert_eq!(ring.len(), 5);

        // Anything with prompt_line < 25 should be dropped.
        ring.prune_below(25);
        let marks: Vec<_> = ring.iter().copied().collect();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].prompt_line, 30);
        assert_eq!(marks[1].prompt_line, 40);
    }

    #[test]
    fn ring_drops_oldest_at_capacity() {
        let mut ring = PromptMarkRing::with_capacity(3);
        for i in 0..5u64 {
            ring.begin_prompt(i * 10);
            ring.mark_done(i * 10 + 1, Some(0), None);
        }
        assert_eq!(ring.len(), 3);
        let lines: Vec<u64> = ring.iter().map(|m| m.prompt_line).collect();
        assert_eq!(lines, vec![20, 30, 40]);
    }

    #[test]
    fn prune_keeps_in_progress_alignment() {
        // Simulate: 3 done marks, then a 4th in-progress. Prune drops
        // the first two. The in-progress index must still point at the
        // 4th mark (now at position 1).
        let mut ring = PromptMarkRing::new();
        ring.begin_prompt(10);
        ring.mark_done(11, Some(0), None);
        ring.begin_prompt(20);
        ring.mark_done(21, Some(0), None);
        ring.begin_prompt(30);
        ring.mark_done(31, Some(0), None);
        ring.begin_prompt(40);
        // In-progress at idx 3; mark_done should land on it.
        ring.prune_below(25);
        // Now ring holds [30/31, 40/in-progress], in_progress = 1.
        ring.mark_done(45, Some(7), None);

        let marks: Vec<_> = ring.iter().copied().collect();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].prompt_line, 30);
        assert_eq!(marks[1].prompt_line, 40);
        assert_eq!(marks[1].done_line, Some(45));
        assert_eq!(marks[1].exit_code, Some(7));
    }
}
