use loom_app::app::{ModalField, ModalKind, kept_set};

use super::App;

impl App {
    /// Single gate for entering a new modal-tier UI state. Closes
    /// every peer modal, restores the pre-search scroll if Search
    /// isn't in the kept set, and tears down any in-flight mouse
    /// drag or text-selection.
    ///
    /// This is the App-level half of the close-peers contract. The
    /// AppModel half (`enter_modal_close_peers_core`) handles the
    /// data-only modal fields. The two are split because:
    ///
    /// * `close_search_restore_scroll` mutates `pane_grids` and
    ///   invalidates the tile cache — both App-level concerns.
    /// * `cancel_pending_mouse_interactions` touches `mouse_left_held`
    ///   and the resize-drag state, which live on App, not AppModel.
    ///
    /// Adding a new caller: pick the `ModalKind` variant (with the
    /// right payload for layered cases — see `ModalKind` doc), call
    /// this, *then* set the field. The structural test in loom-app
    /// pins the contract; sprinkling `field = None` across new
    /// sites should be unnecessary.
    pub(crate) fn enter_modal_close_peers(&mut self, keep: ModalKind) {
        let kept = kept_set(keep);
        if !kept.contains(&ModalField::Search) {
            // Restore pre-search scroll BEFORE the AppModel-level
            // helper runs — even though the core helper deliberately
            // skips `search_state`, restoring scroll touches
            // `pane_grids` which the helper doesn't, so order is
            // immaterial. Doing it first keeps the search-teardown
            // colocated with the rest of the cleanup, easier to read.
            self.close_search_restore_scroll();
        }
        // Reset the context-menu scroll offset whenever we transition
        // into a new context-menu modal — opening a fresh menu (even
        // a different one) should land at the top of its item list.
        if matches!(keep, ModalKind::ContextMenu(_)) {
            self.core.context_menu_scroll_offset = 0;
        }
        self.core.enter_modal_close_peers_core(keep);
        // Always cancel drag / selection: every modal opens a surface
        // (settings backdrop, palette overlay, context menu, paste
        // dialog, search bar) that visually occludes wherever the
        // drag started. Without this teardown, mouse-up commits the
        // selection / resize through the modal.
        self.cancel_pending_mouse_interactions();
        // Visual state changed — request a repaint. Without this,
        // call sites that mutate modal state but don't otherwise
        // schedule redraw (e.g. `Focused(false)` in `event.rs`, the
        // mouse-passthrough early-return in `open_context_menu`)
        // leave stale overlays painted on screen even though input
        // state has been cleared.
        self.schedule_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ScrollbarDragInfo};
    use loom_app::app::{ContextMenuParent, PendingPasteTarget};
    use loom_config::config::LoomConfig;

    fn make() -> App {
        App::new(LoomConfig::default(), "test")
    }

    /// Every variant teardown clears mouse drag + selection. Pinned
    /// because a regression here lets a text-selection drag
    /// extend through a freshly-opened modal — the exact bug R14
    /// caught.
    #[test]
    fn enter_modal_close_peers_always_cancels_mouse_interactions() {
        let cases: &[ModalKind] = &[
            ModalKind::None,
            ModalKind::CommandPalette,
            ModalKind::SessionPalette,
            ModalKind::Settings,
            ModalKind::Help,
            ModalKind::PendingPaste(PendingPasteTarget::Terminal),
            ModalKind::PendingPaste(PendingPasteTarget::CommandPalette),
            ModalKind::PendingPaste(PendingPasteTarget::Search),
            ModalKind::ContextMenu(ContextMenuParent::Standalone),
            ModalKind::ContextMenu(ContextMenuParent::OverSettings),
            ModalKind::Search,
        ];
        for &keep in cases {
            let mut app = make();
            app.mouse_left_held = true;
            app.drag.col_dragging = Some(0);
            app.drag.tile_dragging = Some((0, 0));
            app.drag.scrollbar_dragging = Some(ScrollbarDragInfo {
                pane_id: 0,
                pane_inner_y: 0.0,
                pane_inner_h: 0.0,
                total_lines: 0,
                visible_rows: 0,
            });

            app.enter_modal_close_peers(keep);

            assert!(!app.mouse_left_held, "{:?}: mouse_left_held", keep);
            assert!(app.drag.col_dragging.is_none(), "{:?}: col_dragging", keep);
            assert!(
                app.drag.tile_dragging.is_none(),
                "{:?}: tile_dragging",
                keep
            );
            assert!(
                app.drag.scrollbar_dragging.is_none(),
                "{:?}: scrollbar_dragging",
                keep
            );
        }
    }
}
