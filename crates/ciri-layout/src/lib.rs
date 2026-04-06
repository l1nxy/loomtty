pub mod column;
pub mod geometry;
pub mod tile;
pub mod workspace;
pub mod workspace_set;

/// Shared test utilities for layout tests.
#[cfg(test)]
pub(crate) mod test_util {
    use crate::column::ColumnWidth;
    use crate::tile::PaneId;
    use crate::workspace::Workspace;

    pub const DEFAULT_TEST_WIDTH: ColumnWidth = ColumnWidth::Proportion(0.5);

    pub trait WorkspaceTestExt {
        fn add_test_column(&mut self, pane_id: PaneId);
    }

    impl WorkspaceTestExt for Workspace {
        fn add_test_column(&mut self, pane_id: PaneId) {
            self.add_column_right(pane_id, DEFAULT_TEST_WIDTH);
        }
    }
}
