use crate::tile::PaneId;

/// Width specification for a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    Proportion(f64),
    Fixed(f64),
}

impl Default for ColumnWidth {
    fn default() -> Self {
        ColumnWidth::Proportion(0.5)
    }
}

/// A column is one pane in a horizontal row. No vertical stacking.
#[derive(Debug, Clone)]
pub struct Column {
    pub pane_id: PaneId,
    pub width: ColumnWidth,
    /// Current rendered width in pixels. Set once at creation/resize,
    /// then only changed by explicit animation or jump.
    rendered_width: Option<f32>,
}

impl Column {
    pub fn new(pane_id: PaneId) -> Self {
        Column {
            pane_id,
            width: ColumnWidth::default(),
            rendered_width: None,
        }
    }

    pub fn resolve_width(&self, viewport_w: f32) -> f32 {
        match self.width {
            ColumnWidth::Proportion(p) => (viewport_w as f64 * p) as f32,
            ColumnWidth::Fixed(px) => px as f32,
        }
    }

    pub fn effective_width(&self, viewport_w: f32) -> f32 {
        self.rendered_width.unwrap_or_else(|| self.resolve_width(viewport_w))
    }

    /// Set the rendered width immediately (no animation).
    pub fn snap_width(&mut self, viewport_w: f32) {
        self.rendered_width = Some(self.resolve_width(viewport_w));
    }

    /// Set the rendered width to an explicit pixel value.
    pub fn set_rendered_width(&mut self, w: f32) {
        self.rendered_width = Some(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_width() {
        let col = Column::new(1);
        assert_eq!(col.resolve_width(1000.0), 500.0);
    }

    #[test]
    fn effective_width_uses_rendered() {
        let mut col = Column::new(1);
        col.set_rendered_width(300.0);
        assert_eq!(col.effective_width(1000.0), 300.0);
    }

    #[test]
    fn snap_width_sets_resolve() {
        let mut col = Column::new(1);
        col.snap_width(1000.0);
        assert_eq!(col.effective_width(1000.0), 500.0);
    }
}
