use ciri_anim::spring::SpringParams;

use super::AppModel;

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
            // Close every modal-ish overlay so overview cleanly owns
            // the surface. Without dismissing settings_panel here, the
            // panel's full-viewport backdrop continues to render and
            // `UiFrame::click` / `hover` route every event to the
            // panel's dispatcher before reaching the `overview.active`
            // arm — overview is completely unreachable by mouse until
            // the user closes settings via Esc. Mirrors the
            // close-others discipline of `ToggleSettings` /
            // `open_command_palette`.
            self.context_menu.visible = false;
            self.settings_panel_visible = false;
            self.command_palette = None;
            self.pending_paste = None;
            // Search bar lives on the transient layer and isn't
            // suppressed by modal gates, so leaving `search_state`
            // active alongside overview leaves both widgets visible
            // AND keyboard-routable simultaneously. Clear it the same
            // way `ToggleSettings` does in `action.rs`.
            self.search_state = None;
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
