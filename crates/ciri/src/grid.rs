use ciri_protocol::message::*;

/// Client-side mirror of a pane's cell grid. Pure data, no PTY.
pub struct ClientPaneGrid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<PackedCell>,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    pub title: String,
    pub dirty: bool,
}

impl ClientPaneGrid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let count = cols as usize * rows as usize;
        ClientPaneGrid {
            cols,
            rows,
            cells: vec![PackedCell::default(); count],
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            title: String::new(),
            dirty: true,
        }
    }

    /// Apply a FullPaneSync: replace entire grid.
    pub fn apply_full_sync(&mut self, sync: &FullPaneSync) {
        self.cols = sync.cols;
        self.rows = sync.rows;
        self.cursor_line = sync.cursor_line;
        self.cursor_col = sync.cursor_col;
        self.cursor_shape = sync.cursor_shape;
        self.title = sync.title.clone();
        self.cells = sync.cells.clone();
        self.dirty = true;
    }

    /// Apply incremental CellDelta: patch changed regions in-place.
    pub fn apply_delta(&mut self, delta: &CellDelta) {
        for region in &delta.regions {
            let line = region.line as usize;
            if line >= self.rows as usize { continue; }
            for (i, &cell) in region.cells.iter().enumerate() {
                let col = region.left as usize + i;
                if col >= self.cols as usize { continue; }
                let idx = line * self.cols as usize + col;
                if idx < self.cells.len() {
                    self.cells[idx] = cell;
                }
            }
        }
        self.dirty = true;
    }
}
