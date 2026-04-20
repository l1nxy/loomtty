use crate::column::ColumnWidth;
use crate::geometry::{Rect, ViewSize};
use crate::tile::PaneId;
use crate::workspace::Workspace;

/// A 2D grid of panes: workspaces × columns.
/// Each workspace is a horizontal strip of columns (a Workspace).
/// Vertical axis: workspaces stack downward, each workspace = viewport height.
/// Both axes scroll infinitely.
#[derive(Debug)]
pub struct WorkspaceSet {
    pub workspaces: Vec<Workspace>,
    pub active_workspace_idx: usize,
    pub view_size: ViewSize,
    pub workspace_gap: f32,
    pub column_gap: f32,
}

impl WorkspaceSet {
    pub fn new_with_gaps(view_size: ViewSize, workspace_gap: f32, column_gap: f32) -> Self {
        WorkspaceSet {
            workspaces: vec![Workspace::new_with_gap(view_size, column_gap)],
            active_workspace_idx: 0,
            view_size,
            workspace_gap,
            column_gap,
        }
    }

    pub fn new(view_size: ViewSize) -> Self {
        Self::new_with_gaps(view_size, 8.0, 8.0)
    }

    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active_workspace_idx]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace_idx]
    }

    /// Y position of a workspace in world coordinates.
    pub fn workspace_y(&self, idx: usize) -> f32 {
        idx as f32 * (self.view_size.height + self.workspace_gap)
    }

    /// Target view_offset_y to center the active workspace.
    pub fn target_offset_y(&self) -> f32 {
        let ws_center = self.workspace_y(self.active_workspace_idx) + self.view_size.height / 2.0;
        (ws_center - self.view_size.height / 2.0).max(0.0)
    }

    /// Move focus up one workspace. Tries to keep the same column index.
    pub fn focus_up(&mut self) -> bool {
        if self.active_workspace_idx > 0 {
            let col_idx = self.active().active_column_idx;
            self.active_workspace_idx -= 1;
            // Clamp column index to new workspace's range
            let max = self.workspaces[self.active_workspace_idx]
                .columns
                .len()
                .saturating_sub(1);
            let dst = &mut self.workspaces[self.active_workspace_idx];
            dst.active_column_idx = col_idx.min(max);
            dst.prev_active_column_idx = None;
            return true;
        }
        false
    }

    /// Move focus down one workspace. Tries to keep the same column index.
    pub fn focus_down(&mut self) -> bool {
        if self.active_workspace_idx + 1 < self.workspaces.len() {
            let col_idx = self.active().active_column_idx;
            self.active_workspace_idx += 1;
            let max = self.workspaces[self.active_workspace_idx]
                .columns
                .len()
                .saturating_sub(1);
            let dst = &mut self.workspaces[self.active_workspace_idx];
            dst.active_column_idx = col_idx.min(max);
            dst.prev_active_column_idx = None;
            return true;
        }
        false
    }

    /// Add a pane to the next workspace below, or create one if at the bottom.
    pub fn add_workspace_below(&mut self, pane_id: PaneId) {
        let next = self.active_workspace_idx + 1;
        if next < self.workspaces.len() {
            // Next workspace exists — add a column there and switch to it.
            self.workspaces[next].add_column_right(pane_id, ColumnWidth::Proportion(1.0));
            self.active_workspace_idx = next;
        } else {
            // At the bottom — create a new workspace.
            let mut ws = Workspace::new_with_gap(self.view_size, self.column_gap);
            ws.add_column_right(pane_id, ColumnWidth::Proportion(1.0));
            self.workspaces.push(ws);
            self.active_workspace_idx = next;
        }
    }

    /// Switch to workspace by index, creating workspaces if needed.
    pub fn switch_to(&mut self, idx: usize) {
        while self.workspaces.len() <= idx {
            self.workspaces
                .push(Workspace::new_with_gap(self.view_size, self.column_gap));
        }
        self.active_workspace_idx = idx;
    }

    /// Get ALL visible tiles across all visible workspaces with screen coordinates.
    /// Returns (pane_id, screen_rect, is_active).
    /// `view_offset_x` / `view_offset_y` are the current animated viewport offsets from App.
    pub fn visible_tiles_2d(
        &self,
        view_offset_x: f32,
        view_offset_y: f32,
    ) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let vp_top = view_offset_y;
        let vp_bottom = vp_top + self.view_size.height;

        let active_pane = self.active().active_pane_id();

        for (ws_idx, ws) in self.workspaces.iter().enumerate() {
            let wy = self.workspace_y(ws_idx);
            let ws_h = self.view_size.height;

            if wy + ws_h < vp_top || wy > vp_bottom {
                continue;
            }

            let screen_y = wy - view_offset_y;

            let vp_left = view_offset_x;
            let vp_right = vp_left + self.view_size.width;

            for (col_idx, col) in ws.columns.iter().enumerate() {
                let col_x = ws.column_x(col_idx);
                let col_w = col.effective_width(self.view_size.width);

                if col_x + col_w < vp_left || col_x > vp_right {
                    continue;
                }

                let tile_rects = col.tile_rects(col_w, ws_h);
                for (pane_id, tile_y, tile_h) in &tile_rects {
                    let is_active =
                        ws_idx == self.active_workspace_idx && Some(*pane_id) == active_pane;
                    result.push((
                        *pane_id,
                        Rect::new(col_x - view_offset_x, screen_y + *tile_y, col_w, *tile_h),
                        is_active,
                    ));
                }
            }
        }

        result
    }

    /// Get ALL tiles without culling (for overview).
    /// `view_offset_x` / `view_offset_y` are the current animated viewport offsets from App.
    pub fn all_tiles_2d(
        &self,
        view_offset_x: f32,
        view_offset_y: f32,
    ) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let active_pane = self.active().active_pane_id();

        for (ws_idx, ws) in self.workspaces.iter().enumerate() {
            let wy = self.workspace_y(ws_idx) - view_offset_y;
            let ws_h = self.view_size.height;

            for (col_idx, col) in ws.columns.iter().enumerate() {
                let col_x = ws.column_x(col_idx) - view_offset_x;
                let col_w = col.effective_width(self.view_size.width);
                let tile_rects = col.tile_rects(col_w, ws_h);
                for (pane_id, tile_y, tile_h) in &tile_rects {
                    let is_active =
                        ws_idx == self.active_workspace_idx && Some(*pane_id) == active_pane;
                    result.push((
                        *pane_id,
                        Rect::new(col_x, wy + *tile_y, col_w, *tile_h),
                        is_active,
                    ));
                }
            }
        }
        result
    }

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.workspaces
            .iter()
            .flat_map(|r| r.all_pane_ids())
            .collect()
    }

    pub fn resize_view(&mut self, size: ViewSize) {
        self.view_size = size;
        for ws in &mut self.workspaces {
            ws.resize_view(size);
        }
    }

    /// Remove empty workspaces (keep at least one).
    /// If the active workspace is removed, focus moves to the workspace above (or below if at top).
    pub fn cleanup_empty(&mut self) {
        // First: if the active workspace itself is empty, move focus before cleanup
        if self.workspaces.len() > 1 && self.workspaces[self.active_workspace_idx].is_empty() {
            if self.active_workspace_idx > 0 {
                self.active_workspace_idx -= 1;
            } else {
                // active workspace is 0 and empty — find the first non-empty workspace
                for i in 1..self.workspaces.len() {
                    if !self.workspaces[i].is_empty() {
                        self.active_workspace_idx = i;
                        break;
                    }
                }
            }
        }

        // Now remove all empty workspaces (except keep at least one)
        let mut i = 0;
        while i < self.workspaces.len() && self.workspaces.len() > 1 {
            if self.workspaces[i].is_empty() {
                self.workspaces.remove(i);
                if self.active_workspace_idx > i {
                    self.active_workspace_idx -= 1;
                } else if self.active_workspace_idx == i {
                    // Shouldn't happen after the fix above, but clamp defensively
                    self.active_workspace_idx = self
                        .active_workspace_idx
                        .min(self.workspaces.len().saturating_sub(1));
                }
            } else {
                i += 1;
            }
        }
    }

    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::WorkspaceTestExt;
    use crate::workspace::CenterStrategy;

    fn wss() -> WorkspaceSet {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        ws.active_mut().add_test_column(1);
        ws
    }

    #[test]
    fn focus_up_down_navigates_workspaces() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.add_workspace_below(3);
        assert_eq!(ws.active_workspace_idx, 2);
        ws.focus_up();
        assert_eq!(ws.active_workspace_idx, 1);
        ws.focus_down();
        assert_eq!(ws.active_workspace_idx, 2);
    }

    #[test]
    fn focus_preserves_column_index() {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        // Workspace 0: 3 columns
        ws.active_mut().add_test_column(1);
        ws.active_mut().add_test_column(2);
        ws.active_mut().add_test_column(3);
        // active_column_idx = 2 (rightmost)

        // Workspace 1: 2 columns
        ws.add_workspace_below(4);
        ws.active_mut().add_test_column(5);
        // active_column_idx = 1

        // Go back to workspace 0 — should restore column 1 (clamped from 1)
        ws.focus_up();
        assert_eq!(ws.active_workspace_idx, 0);
        assert_eq!(ws.active().active_column_idx, 1); // clamped from workspace 1's index

        // Go to workspace 1
        ws.focus_down();
        assert_eq!(ws.active_workspace_idx, 1);
    }

    #[test]
    fn visible_tiles_2d_basic() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        // Both workspaces visible when view_offset_y = 0 (workspaces are at y=0 and y=608)
        // But workspace 1 at y=608 > viewport height 600, so it's off-screen
        let tiles = ws.visible_tiles_2d(0.0, 0.0);
        // Only workspace 0 is visible (its pane 1)... wait, workspace 0 has pane 1 but view_offset_y=0
        // and workspace 1 is at y=608 which is > 600, so not visible
        assert_eq!(tiles.len(), 1);
    }

    #[test]
    fn target_offset_y() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        // active_workspace_idx = 1, workspace_y(1) = 608
        let target = ws.target_offset_y();
        assert_eq!(target, 608.0); // center of workspace 1 = 608+300=908, - 300 = 608
    }

    #[test]
    fn all_tiles_2d() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.active_mut().add_test_column(3);
        let all = ws.all_tiles_2d(0.0, 0.0);
        assert_eq!(all.len(), 3); // workspace 0: pane 1, workspace 1: pane 2 + pane 3
    }

    #[test]
    fn cleanup_empty_focuses_previous_workspace() {
        let mut ws = wss(); // workspace 0 has pane 1
        ws.add_workspace_below(2); // workspace 1 has pane 2, active_workspace_idx = 1
        ws.add_workspace_below(3); // workspace 2 has pane 3, active_workspace_idx = 2
        assert_eq!(ws.active_workspace_idx, 2);

        // Close the pane in active workspace (workspace 2) — making it empty
        ws.active_mut().close_pane(3);
        assert!(ws.workspaces[2].is_empty());

        // cleanup_empty should remove workspace 2 and focus workspace 1
        ws.cleanup_empty();
        assert_eq!(ws.workspaces.len(), 2);
        assert_eq!(ws.active_workspace_idx, 1);
        assert_eq!(ws.active().active_pane_id(), Some(2));
    }

    #[test]
    fn cleanup_empty_active_workspace_0() {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        ws.active_mut().add_test_column(1); // workspace 0
        ws.add_workspace_below(2); // workspace 1, active
        ws.active_workspace_idx = 0; // switch back to workspace 0
        ws.active_mut().close_pane(1); // workspace 0 is now empty

        ws.cleanup_empty();
        assert_eq!(ws.workspaces.len(), 1);
        assert_eq!(ws.active_workspace_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(2));
    }

    #[test]
    fn target_offset_clamps_stale_active_column_before_viewport_targeting() {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        ws.active_mut().add_test_column(1);
        ws.active_mut().add_test_column(2);
        ws.active_mut().add_test_column(3);
        ws.active_mut().active_column_idx = 2;

        ws.add_workspace_below(4);
        assert_eq!(ws.active().active_column_idx, 0);
        ws.active_mut().add_test_column(5);
        assert_eq!(ws.active().active_column_idx, 1);
        ws.active_mut().close_pane(5);

        ws.focus_up();
        assert_eq!(ws.active_workspace_idx, 0);
        assert_eq!(ws.active().active_column_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(1));
        assert_eq!(
            ws.active()
                .target_offset_for_active_with_strategy(CenterStrategy::Always, 0.0),
            0.0
        );

        ws.add_workspace_below(6);
        ws.active_mut().close_pane(6);
        ws.cleanup_empty();

        assert_eq!(ws.workspaces.len(), 2);
        assert_eq!(ws.active_workspace_idx, 1);
        assert_eq!(ws.active().columns.len(), 1);
        assert_eq!(ws.active().active_column_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(4));
        assert_eq!(
            ws.active()
                .target_offset_for_active_with_strategy(CenterStrategy::Always, 0.0),
            0.0
        );
    }

    #[test]
    fn cleanup_empty_clamps_active_column_and_visible_state() {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        ws.active_mut().add_test_column(1);
        ws.active_mut().add_test_column(2);
        ws.active_mut().add_test_column(3);
        ws.active_mut().active_column_idx = 2;

        ws.add_workspace_below(4);
        ws.active_mut().add_test_column(5);
        ws.active_mut().close_pane(5);

        ws.focus_up();
        assert_eq!(ws.active_workspace_idx, 0);
        assert_eq!(ws.active().active_column_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(1));

        ws.add_workspace_below(6);
        ws.active_mut().close_pane(6);
        ws.cleanup_empty();

        assert_eq!(ws.workspaces.len(), 2);
        assert_eq!(ws.active_workspace_idx, 1);
        assert_eq!(ws.active().columns.len(), 1);
        assert_eq!(ws.active().active_column_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(4));

        let visible = ws.visible_tiles_2d(0.0, ws.target_offset_y());
        let active_visible: Vec<_> = visible
            .iter()
            .filter(|(_, _, is_active)| *is_active)
            .collect();
        assert_eq!(active_visible.len(), 1);
        assert_eq!(active_visible[0].0, 4);

        let all = ws.all_tiles_2d(0.0, ws.target_offset_y());
        let active_tiles: Vec<_> = all
            .into_iter()
            .filter(|(_, _, is_active)| *is_active)
            .collect();
        assert_eq!(active_tiles.len(), 1);
        assert_eq!(active_tiles[0].0, 4);
    }

    // ── Resize propagation ──────────────────────────────────────────

    #[test]
    fn resize_view_propagates_to_all_workspaces() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.add_workspace_below(3);
        assert_eq!(ws.workspaces.len(), 3);

        let new_size = ViewSize {
            width: 1920.0,
            height: 1080.0,
        };
        ws.resize_view(new_size);

        assert_eq!(ws.view_size, new_size);
        for w in &ws.workspaces {
            assert_eq!(w.view_size, new_size);
        }
    }

    #[test]
    fn resize_view_updates_workspace_y_positions() {
        let mut ws = wss();
        ws.add_workspace_below(2);

        let y_before = ws.workspace_y(1);
        ws.resize_view(ViewSize {
            width: 1000.0,
            height: 1200.0,
        });
        let y_after = ws.workspace_y(1);

        // workspace_y = idx * (height + gap), so it changes with height
        assert_ne!(y_before, y_after);
        assert_eq!(y_after, 1200.0 + ws.workspace_gap);
    }

    #[test]
    fn resize_view_tiny_size() {
        let mut ws = wss();
        ws.add_workspace_below(2);

        ws.resize_view(ViewSize {
            width: 1.0,
            height: 1.0,
        });

        // Should not panic
        let tiles = ws.visible_tiles_2d(0.0, ws.target_offset_y());
        assert!(!tiles.is_empty());
    }

    // ── switch_to ───────────────────────────────────────────────────

    #[test]
    fn switch_to_creates_intermediate_workspaces() {
        let mut ws = wss();
        ws.switch_to(5);
        assert_eq!(ws.workspaces.len(), 6);
        assert_eq!(ws.active_workspace_idx, 5);
        // Intermediate workspaces should be empty
        for i in 1..5 {
            assert!(ws.workspaces[i].is_empty());
        }
    }

    #[test]
    fn switch_to_existing_workspace() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.add_workspace_below(3);
        ws.switch_to(0);
        assert_eq!(ws.active_workspace_idx, 0);
        assert_eq!(ws.active().active_pane_id(), Some(1));
    }

    // ── Multi-workspace navigation ──────────────────────────────────

    #[test]
    fn focus_up_at_top_returns_false() {
        let mut ws = wss();
        assert!(!ws.focus_up());
        assert_eq!(ws.active_workspace_idx, 0);
    }

    #[test]
    fn focus_down_at_bottom_returns_false() {
        let mut ws = wss();
        assert!(!ws.focus_down());
        assert_eq!(ws.active_workspace_idx, 0);
    }

    #[test]
    fn add_workspace_below_reuses_existing_empty() {
        let mut ws = wss();
        ws.add_workspace_below(2); // ws[1]
        ws.active_workspace_idx = 0; // go back to ws[0]
        ws.active_mut().close_pane(1); // make ws[0] empty...wait, ws[0] still has pane 1

        // Actually: let's add a workspace, go back up, then add_workspace_below
        // should reuse the existing ws[1] if we're at ws[0]
        let ws_count_before = ws.workspaces.len();
        ws.add_workspace_below(3); // should add to existing ws[1]
        assert_eq!(ws.workspaces.len(), ws_count_before); // no new workspace
        assert_eq!(ws.active_workspace_idx, 1);
    }

    // ── Visibility culling ──────────────────────────────────────────

    #[test]
    fn visible_tiles_2d_scrolled_to_second_workspace() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        // Scroll to workspace 1
        let offset_y = ws.target_offset_y();
        let tiles = ws.visible_tiles_2d(0.0, offset_y);
        // Should see workspace 1's tiles
        let pane_ids: Vec<u64> = tiles.iter().map(|(id, _, _)| *id).collect();
        assert!(pane_ids.contains(&2));
    }

    #[test]
    fn all_pane_ids_across_workspaces() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.active_mut().add_test_column(3);
        let ids = ws.all_pane_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    // ── Cleanup edge cases ──────────────────────────────────────────

    #[test]
    fn cleanup_empty_keeps_at_least_one_workspace() {
        let mut ws = WorkspaceSet::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        });
        // Don't add any panes — workspace 0 is empty
        ws.cleanup_empty();
        assert_eq!(ws.workspaces.len(), 1);
    }

    #[test]
    fn cleanup_multiple_empty_workspaces() {
        let mut ws = wss();
        ws.add_workspace_below(2);
        ws.add_workspace_below(3);
        ws.add_workspace_below(4);
        // Close panes in ws[1] and ws[2], keep ws[0] and ws[3]
        ws.workspaces[1].close_pane(2);
        ws.workspaces[2].close_pane(3);
        assert!(ws.workspaces[1].is_empty());
        assert!(ws.workspaces[2].is_empty());

        ws.cleanup_empty();
        // Only ws[0] (pane 1) and ws[3] (pane 4) should remain
        assert_eq!(ws.workspaces.len(), 2);
        let all_ids = ws.all_pane_ids();
        assert!(all_ids.contains(&1));
        assert!(all_ids.contains(&4));
    }
}
