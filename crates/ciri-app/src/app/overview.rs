use ciri_anim::spring::SpringParams;

use super::{AppModel, ModalKind};

impl AppModel {
    pub fn exit_overview(&mut self) {
        let sp = SpringParams::default();
        self.overview.active = false;
        // Hover state lives on `App` (UI shell) — bin-crate handlers
        // reset it on the same event paths that call `exit_overview`.
        self.anim_mgr.overview_zoom.animate_to(1.0, sp);
        self.animate_to_active();
    }

    pub fn toggle_overview(&mut self) {
        self.overview.active = !self.overview.active;
        let sp = SpringParams::default();
        if self.overview.active {
            // Close every modal-tier overlay so overview cleanly owns
            // the surface. Without dismissing settings_panel here, its
            // full-viewport backdrop continues to render and
            // `UiFrame::click` / `hover` route every event to the
            // panel before reaching the `overview.active` arm —
            // overview becomes mouse-unreachable until the user
            // closes settings via Esc. Production callers route
            // through `App::toggle_overview` which runs
            // `close_search_restore_scroll` (the only correct way to
            // tear down `search_state` — it requires `pane_grids`
            // mutation that AppModel can't reach). DO NOT call this
            // method directly without the App wrapper; an active
            // search session would silently lose its pre-search
            // scroll snapshot.
            self.enter_modal_close_peers_core(ModalKind::None);
            self.refresh_overview_zoom();
            let center = match self.config.layout.center_focused_column {
                ciri_config::config::CenterStrategy::Always => {
                    ciri_layout::workspace::CenterStrategy::Always
                }
                ciri_config::config::CenterStrategy::OnOverflow => {
                    ciri_layout::workspace::CenterStrategy::OnOverflow
                }
                ciri_config::config::CenterStrategy::Never => {
                    ciri_layout::workspace::CenterStrategy::Never
                }
            };
            let current_x = self.anim_mgr.view_offset_x.value() as f32;
            let target_x = self
                .workspaces
                .active()
                .target_offset_for_active_with_strategy(center, current_x) as f64;
            let target_y = self.workspaces.target_offset_y() as f64;
            self.anim_mgr.view_offset_x.animate_to(target_x, sp);
            self.anim_mgr.view_offset_y.animate_to(target_y, sp);
        } else {
            self.anim_mgr.overview_zoom.animate_to(1.0, sp);
            self.animate_to_active();
        }
    }
}
