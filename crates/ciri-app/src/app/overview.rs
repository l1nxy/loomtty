use ciri_anim::spring::SpringParams;

use super::AppModel;

impl AppModel {
    pub fn exit_overview(&mut self) {
        let sp = SpringParams::default();
        self.overview.active = false;
        self.overview.hovered_pane = None;
        self.anim_mgr.overview_zoom.animate_to(1.0, sp);
        self.animate_to_active();
    }

    pub fn toggle_overview(&mut self) {
        self.overview.active = !self.overview.active;
        let sp = SpringParams::default();
        if self.overview.active {
            self.context_menu.visible = false;
            self.overview.hovered_pane = None;
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
            self.overview.hovered_pane = None;
            self.anim_mgr.overview_zoom.animate_to(1.0, sp);
            self.animate_to_active();
        }
    }
}
