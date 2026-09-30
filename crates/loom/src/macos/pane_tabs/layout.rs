use std::ops::Range;

pub(super) const MIN_WIDTH: f64 = 80.0;
pub(super) const MAX_WIDTH: f64 = 220.0;
const LABEL_PADDING: f64 = 28.0;

/// Prefix sums keep hit positions and virtualization independent of pane count.
#[derive(Default)]
pub(super) struct Layout {
    edges: Vec<f64>,
}

impl Layout {
    pub fn measured(widths: impl IntoIterator<Item = f64>) -> Self {
        let mut edges = vec![0.0];
        for width in widths {
            let width = if width.is_finite() { width } else { 0.0 };
            edges.push(edges.last().unwrap() + (width + LABEL_PADDING).clamp(MIN_WIDTH, MAX_WIDTH));
        }
        Self { edges }
    }

    pub fn total(&self) -> f64 {
        self.edges.last().copied().unwrap_or(0.0)
    }
    pub fn start(&self, index: usize) -> f64 {
        self.edges[index]
    }
    pub fn width(&self, index: usize) -> f64 {
        self.edges[index + 1] - self.edges[index]
    }

    pub fn visible(&self, offset: f64, viewport: f64) -> Range<usize> {
        let count = self.edges.len().saturating_sub(1);
        if count == 0 {
            return 0..0;
        }
        // One maximum-width tab of overscan covers native momentum scrolling.
        let left = (offset - MAX_WIDTH).max(0.0);
        let right = (offset + viewport.max(0.0) + MAX_WIDTH).min(self.total());
        let start = self
            .edges
            .partition_point(|&x| x <= left)
            .saturating_sub(1)
            .min(count);
        let end = self.edges.partition_point(|&x| x < right).min(count);
        start..end.max(start)
    }

    pub fn reveal(&self, index: usize, offset: f64, viewport: f64) -> f64 {
        let start = self.start(index);
        let end = start + self.width(index);
        let next = if start < offset || self.width(index) > viewport {
            start
        } else if end > offset + viewport {
            end - viewport
        } else {
            offset
        };
        next.clamp(0.0, (self.total() - viewport).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_are_bounded_and_long_titles_do_not_expand_the_viewport() {
        let layout = Layout::measured([0.0, 90.0, 100_000.0]);
        assert_eq!(layout.width(0), MIN_WIDTH);
        assert_eq!(layout.width(1), 118.0);
        assert_eq!(layout.width(2), MAX_WIDTH);
        assert_eq!(layout.reveal(2, 0.0, 300.0), 118.0);
        assert_eq!(layout.reveal(2, 0.0, 100.0), 198.0);
    }

    #[test]
    fn ten_thousand_panes_keep_native_control_count_bounded() {
        let layout = Layout::measured(std::iter::repeat_n(60.0, 10_000));
        for pane in [0, 1, 5000, 9999] {
            let offset = layout.reveal(pane, 0.0, 900.0);
            let range = layout.visible(offset, 900.0);
            assert!(range.contains(&pane));
            assert!(
                range.len() <= 18,
                "native controls must scale with viewport width"
            );
            assert!(offset + 900.0 <= layout.total());
        }
        let middle = layout.visible(400_000.0, 900.0);
        assert!(middle.start > 0 && middle.end < 10_000);
    }

    #[test]
    fn resizing_and_removal_clamp_scroll_without_losing_selection() {
        let layout = Layout::measured([50.0; 20]);
        let offset = layout.reveal(19, 0.0, 500.0);
        let narrower = layout.reveal(19, offset, 150.0);
        assert_eq!(narrower, layout.total() - 150.0);
        let reduced = Layout::measured([50.0; 2]);
        assert_eq!(reduced.reveal(1, narrower, 500.0), 0.0);
        assert_eq!(Layout::default().visible(0.0, 500.0), 0..0);
    }
}
