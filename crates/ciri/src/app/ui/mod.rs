#[allow(dead_code)] // cursor-based widgets available for future component migration
pub(crate) mod builder;
mod context_menu;
mod hints_bar;
pub(super) mod info_box;
mod overview;
mod palette;
mod paste_dialog;
mod top_bar;
pub(crate) mod types;

pub(crate) use context_menu::ContextMenuComponent;
pub(crate) use hints_bar::HintsBarComponent;
pub(crate) use info_box::InfoBoxComponent;
pub(crate) use overview::OverviewComponent;
pub(crate) use palette::PaletteComponent;
pub(crate) use paste_dialog::PasteDialogComponent;
pub(crate) use top_bar::TopBarComponent;
pub(crate) use types::*;

use ciri_render::glyph_cache::GlyphInstance;
use ciri_render::rect::Rect;
use winit::window::CursorIcon;

use super::{App, TopBarHoverRegion};

impl App {
    pub(crate) fn build_ui(
        &mut self,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let cell_w = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_width)
            .unwrap_or(8.0);
        let top_bar_layout = self.top_bar_layout(vw, vh, cell_w, cell_h);
        self.ensure_active_pane_tab_visible(top_bar_layout.tabs_area_px);
        let baseline = cell_h * self.core.config.statusbar.text_baseline;
        let cx = UiContext {
            config: &self.core.config,
            viewport_w: vw,
            viewport_h: vh,
            cell_w,
            cell_h,
            baseline,
        };

        let top_bar = TopBarComponent::capture(self, top_bar_layout, &cx);
        let hints_bar = HintsBarComponent::capture(self, &cx);
        let palette = PaletteComponent::capture(self, &cx);
        let context_menu = ContextMenuComponent::capture(self, &cx);
        let paste_dialog = PasteDialogComponent::capture(self, &cx);
        let infobox = InfoBoxComponent::capture(self, &cx);
        let overview_bar = if self.core.overview.active && self.core.overview.hovered_pane.is_some()
        {
            overview::overview_action_bar_data(self, self.core.overview.hovered_pane)
        } else {
            None
        };
        let overview_hover = self.core.overview_action_hover;

        let atlas = self.glyph_cache.as_mut().unwrap();
        let mut scene = UiScene {
            atlas,
            bg_rects,
            glyphs,
            color_glyphs,
        };

        top_bar.paint(&cx, &mut scene);
        hints_bar.paint(&cx, &mut scene);
        if let Some(d) = &overview_bar {
            overview::paint_overview_action_bar(d, overview_hover, &cx, &mut scene);
        }
        if let Some(component) = infobox {
            component.paint(&cx, &mut scene);
        }
        if let Some(component) = palette {
            component.paint(&cx, &mut scene);
        }
        if let Some(component) = paste_dialog {
            component.paint(&cx, &mut scene);
        }
        if let Some(component) = context_menu {
            component.paint(&cx, &mut scene);
        }
    }

    pub(crate) fn ui_context(&self) -> UiContext<'_> {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let cell_w = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_width)
            .unwrap_or(8.0);
        let (viewport_w, viewport_h) = self.command_palette_viewport_size();
        UiContext {
            config: &self.core.config,
            viewport_w,
            viewport_h,
            cell_w,
            cell_h,
            baseline: cell_h * self.core.config.statusbar.text_baseline,
        }
    }

    pub(crate) fn ui_top_bar_hover(
        &self,
        mx: f32,
        my: f32,
    ) -> (Option<TopBarHoverRegion>, Option<u64>) {
        let cx = self.ui_context();
        let component = TopBarComponent::capture(
            self,
            self.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h),
            &cx,
        );
        match component.hit_test(mx, my, &cx) {
            Some(UiTopBarHit::Session) => (Some(TopBarHoverRegion::Session), None),
            Some(UiTopBarHit::Workspace) => (Some(TopBarHoverRegion::Workspace), None),
            Some(UiTopBarHit::Mode) => (Some(TopBarHoverRegion::Mode), None),
            Some(UiTopBarHit::PaneTab(pane_id)) => (None, Some(pane_id)),
            Some(UiTopBarHit::Background) | None => (None, None),
        }
    }


    pub(crate) fn ui_palette_hover(&mut self, mx: f32, my: f32) -> (Option<usize>, bool) {
        let cx = self.ui_context();
        let Some(component) = PaletteComponent::capture(self, &cx) else {
            return (None, false);
        };
        match component.hit_test(mx, my) {
            UiPaletteHit::Entry(entry_idx) => {
                let hovered =
                    self.core.command_palette.as_ref().and_then(|palette| {
                        palette.filtered.iter().position(|&idx| idx == entry_idx)
                    });
                (hovered, true)
            }
            UiPaletteHit::Panel => (None, false),
            UiPaletteHit::None => (None, false),
        }
    }


    pub(crate) fn ui_context_menu_hover(&self, mx: f32, my: f32) -> Option<usize> {
        let cx = self.ui_context();
        let component = ContextMenuComponent::capture(self, &cx)?;
        match component.hit_test(mx, my) {
            UiContextMenuHit::Entry(idx) => Some(idx),
            UiContextMenuHit::Menu | UiContextMenuHit::None => None,
        }
    }

    pub(crate) fn ui_paste_dialog_hover(&self, mx: f32, my: f32) -> Option<super::PasteButton> {
        let cx = self.ui_context();
        let component = PasteDialogComponent::capture(self, &cx)?;
        match component.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(super::PasteButton::Paste),
            UiPasteDialogHit::Cancel => Some(super::PasteButton::Cancel),
            UiPasteDialogHit::Dialog | UiPasteDialogHit::None => None,
        }
    }

    pub(crate) fn ui_overview_hover(&mut self, mx: f32, my: f32) -> Option<(usize, u64)> {
        let cx = self.ui_context();
        let component = OverviewComponent::capture(self, &cx);
        let hit = component.hit_test(self, mx, my);
        self.core.overview_action_hover = match &hit {
            UiOverviewHit::ClosePane(_) => Some(super::OverviewActionHover::Close),
            UiOverviewHit::FocusPane(_, _) => Some(super::OverviewActionHover::Focus),
            _ => None,
        };
        match hit {
            UiOverviewHit::Pane(ws_idx, pane_id) | UiOverviewHit::FocusPane(ws_idx, pane_id) => {
                Some((ws_idx, pane_id))
            }
            UiOverviewHit::ClosePane(_) => self.core.overview.hovered_pane,
            UiOverviewHit::Background | UiOverviewHit::None => None,
        }
    }

    fn ui_overview_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let component = OverviewComponent::capture(self, &cx);
        match component.hit_test(self, mx, my) {
            UiOverviewHit::Pane(ws_idx, pane_id) => {
                Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
            }
            UiOverviewHit::FocusPane(ws_idx, pane_id) => {
                Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
            }
            UiOverviewHit::ClosePane(pane_id) => Some(UiAction::CloseOverviewPane(pane_id)),
            UiOverviewHit::Background => Some(UiAction::StartOverviewDrag),
            UiOverviewHit::None => None,
        }
    }

    pub(crate) fn dispatch_ui_click(&mut self, mx: f32, my: f32) -> bool {
        let cx = self.ui_context();

        // Components are checked in z-order (highest priority first).
        // The FIRST component that handles the click wins — no further checks.

        // 1. Modal overlays (consume ALL input when active)
        if let Some(c) = PasteDialogComponent::capture(self, &cx) {
            if let Some(action) = c.click(mx, my, &cx) {
                self.apply_ui_action(action);
            }
            return true; // modal: always consumed
        }
        if let Some(c) = PaletteComponent::capture(self, &cx) {
            if let Some(action) = c.click(mx, my, &cx) {
                self.apply_ui_action(action);
            }
            return true; // modal: always consumed
        }
        if let Some(c) = ContextMenuComponent::capture(self, &cx) {
            if let Some(action) = c.click(mx, my, &cx) {
                self.apply_ui_action(action);
            }
            return true; // modal: always consumed
        }

        // 2. Top bar
        if self.hit_test_top_bar(mx, my) {
            let layout = self.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h);
            let c = TopBarComponent::capture(self, layout, &cx);
            if let Some(action) = c.click(mx, my, &cx) {
                self.apply_ui_action(action);
                return true;
            }
            return true; // top bar area consumed
        }

        // 3. Overview (special — needs &App for tile hit test)
        if self.core.overview.active {
            if let Some(action) = self.ui_overview_action(mx, my) {
                self.apply_ui_action(action);
            }
            return true; // overview consumes all clicks when active
        }

        false
    }

    pub(crate) fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::OpenSessionPalette => self.open_session_palette(),
            UiAction::ToggleOverview => {
                self.handle_action(ciri_input::action::Action::ToggleOverview);
            }
            UiAction::CycleWorkspace => {
                let workspace_count = self.core.workspaces.workspaces.len();
                if workspace_count > 0 {
                    let next_idx =
                        (self.core.workspaces.active_workspace_idx + 1) % workspace_count;
                    self.core.workspaces.active_workspace_idx = next_idx;
                    if let Some(&pane_id) = self.core.workspace_last_pane_ids.get(&next_idx)
                        && self.focus_workspace_pane_local(next_idx, pane_id)
                    {
                        self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
                    }
                    self.animate_to_active();
                    self.send(ciri_protocol::message::ClientMessage::SwitchWorkspace {
                        workspace_idx: next_idx,
                    });
                }
            }
            UiAction::FocusPaneTab(pane_id) => {
                let mut target: Option<(usize, usize, usize)> = None;
                for (ws_idx, ws) in self.core.workspaces.workspaces.iter().enumerate() {
                    for (col_idx, col) in ws.columns.iter().enumerate() {
                        if col.contains_pane(pane_id) {
                            let tile_idx = col
                                .tiles
                                .iter()
                                .position(|t| t.pane_id == pane_id)
                                .unwrap_or(0);
                            target = Some((ws_idx, col_idx, tile_idx));
                            break;
                        }
                    }
                    if target.is_some() {
                        break;
                    }
                }
                if let Some((ws_idx, col_idx, tile_idx)) = target {
                    self.core.workspaces.active_workspace_idx = ws_idx;
                    let ws = self.core.workspaces.active_mut();
                    ws.active_column_idx = col_idx;
                    if col_idx < ws.columns.len() {
                        ws.columns[col_idx].active_tile_idx =
                            tile_idx.min(ws.columns[col_idx].tiles.len().saturating_sub(1));
                    }
                    self.remember_workspace_pane(ws_idx, pane_id);
                    self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
                    self.animate_to_active();
                }
            }
            UiAction::ExecutePaletteEntry(entry_idx) => {
                // SectionHeaders should not be clickable (hit_test returns Panel),
                // but guard defensively.
                let is_selectable = self
                    .core
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.entries.get(entry_idx))
                    .is_some_and(|e| e.kind.is_selectable());
                if !is_selectable {
                    return;
                }
                let keep_open = self
                    .core
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.entries.get(entry_idx))
                    .is_some_and(|e| {
                        matches!(
                            e.kind,
                            super::PaletteEntryKind::RemoteHost { .. }
                                | super::PaletteEntryKind::ConnectRemotePrompt
                        )
                    });
                if let Some(palette) = &mut self.core.command_palette
                    && let Some(pos) = palette.filtered.iter().position(|&idx| idx == entry_idx)
                {
                    palette.selected_idx = pos;
                }
                self.execute_palette_entry(entry_idx);
                if !keep_open {
                    self.core.command_palette = None;
                }
            }
            UiAction::ClosePalette => self.core.command_palette = None,
            UiAction::ExecuteContextMenuEntry(idx) => {
                self.core.context_menu.hovered_index = Some(idx);
                self.handle_context_menu_click();
            }
            UiAction::CloseContextMenu => self.core.context_menu.visible = false,
            UiAction::ConfirmPaste => self.confirm_pending_paste(),
            UiAction::CancelPaste => self.core.pending_paste = None,
            UiAction::FocusOverviewPane(ws_idx, pane_id) => {
                self.focus_overview_target(ws_idx, pane_id);
            }
            UiAction::CloseOverviewPane(pane_id) => {
                self.core.overview.hovered_pane = None;
                self.send(ciri_protocol::message::ClientMessage::ClosePane { pane_id });
            }
            UiAction::StartOverviewDrag => {
                self.core.overview.dragging = true;
                self.core.overview.drag_last_pos = self.last_mouse_pos;
            }
        }
    }

    pub(crate) fn dispatch_ui_hover(&mut self, mx: f32, my: f32) -> UiHoverOutcome {
        if self.core.pending_paste.is_some() {
            let prev = self
                .core
                .pending_paste
                .as_ref()
                .and_then(|p| p.hovered_button);
            let next = self.ui_paste_dialog_hover(mx, my);
            if let Some(pending) = &mut self.core.pending_paste {
                pending.hovered_button = next;
            }
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: prev != next,
            };
        }

        if self.core.context_menu.visible {
            let prev = self.core.context_menu.hovered_index;
            let next = self.ui_context_menu_hover(mx, my);
            self.core.context_menu.hovered_index = next;
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: prev != next,
            };
        }

        if self.core.command_palette.is_some() {
            let prev_hovered = self
                .core
                .command_palette
                .as_ref()
                .and_then(|p| p.hovered_idx);
            let (next_hovered, pointer) = self.ui_palette_hover(mx, my);
            if let Some(palette) = &mut self.core.command_palette {
                palette.hovered_idx = next_hovered;
            }
            return UiHoverOutcome {
                handled: true,
                cursor: if pointer {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: prev_hovered != next_hovered,
            };
        }

        if self.hit_test_top_bar(mx, my) {
            let prev_region = self.core.hovered_top_bar_region;
            let prev_tab = self.core.hovered_pane_tab;
            let (region, tab) = self.ui_top_bar_hover(mx, my);
            self.core.hovered_top_bar_region = region;
            self.core.hovered_pane_tab = tab;
            return UiHoverOutcome {
                handled: true,
                cursor: if region.is_some() || tab.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: prev_region != region || prev_tab != tab,
            };
        }

        let had_top_bar_hover = self.core.hovered_top_bar_region.take().is_some()
            || self.core.hovered_pane_tab.take().is_some();

        if self.core.overview.active {
            let prev = self.core.overview.hovered_pane;
            let next = self.ui_overview_hover(mx, my);
            self.core.overview.hovered_pane = next;
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: had_top_bar_hover || prev != next,
            };
        }

        UiHoverOutcome {
            handled: had_top_bar_hover,
            cursor: CursorIcon::Default,
            needs_redraw: had_top_bar_hover,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::paste_guard::PasteInfo;
    use crate::app::{ContextMenu, ContextMenuAction, ContextMenuItem, PendingPaste};
    use ciri_app::app::CommandPaletteState;
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn top_bar_session_click_maps_to_open_session_palette() {
        let app = make_app();
        let cx = app.ui_context();
        let layout = app.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h);
        let component = TopBarComponent::capture(&app, layout, &cx);
        let action = component.click(layout.session_x + 4.0, layout.bar_y + 2.0, &cx);
        assert_eq!(action, Some(UiAction::OpenSessionPalette));
    }

    #[test]
    fn context_menu_entry_click_maps_to_execute_entry() {
        let mut app = make_app();
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 40.0,
            y: 50.0,
            target_pane_id: None,
            items: vec![ContextMenuItem {
                label: "Copy".into(),
                action: ContextMenuAction::Copy,
                enabled: true,
            }],
            hovered_index: None,
        };
        let cx = app.ui_context();
        let component = ContextMenuComponent::capture(&app, &cx).unwrap();
        let action = component.click(60.0, 65.0, &cx);
        assert_eq!(action, Some(UiAction::ExecuteContextMenuEntry(0)));
    }

    #[test]
    fn paste_dialog_outside_click_maps_to_cancel() {
        let mut app = make_app();
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 5,
                line_count: 1,
            },
            preview: "hello".into(),
            hovered_button: None,
        });
        let cx = app.ui_context();
        let component = PasteDialogComponent::capture(&app, &cx).unwrap();
        let action = component.click(0.0, 0.0, &cx);
        assert_eq!(action, Some(UiAction::CancelPaste));
    }

    #[test]
    fn overview_background_click_maps_to_start_drag() {
        let mut app = make_app();
        app.core.overview.active = true;
        let cx = app.ui_context();
        let component = OverviewComponent::capture(&app, &cx);
        let hit = component.hit_test(&app, 10.0, 10.0);
        let action = match hit {
            UiOverviewHit::Background => Some(UiAction::StartOverviewDrag),
            _ => None,
        };
        assert_eq!(action, Some(UiAction::StartOverviewDrag));
    }

    #[test]
    fn dispatch_ui_click_closes_palette() {
        let mut app = make_app();
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        assert!(app.dispatch_ui_click(0.0, 0.0));
        assert!(app.core.command_palette.is_none());
    }

    #[test]
    fn dispatch_ui_hover_marks_top_bar_session_as_pointer() {
        let mut app = make_app();
        let cx = app.ui_context();
        let layout = app.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h);
        let hover = app.dispatch_ui_hover(layout.session_x + 2.0, layout.bar_y + 2.0);
        assert!(hover.handled);
        assert_eq!(hover.cursor, CursorIcon::Pointer);
        assert_eq!(
            app.core.hovered_top_bar_region,
            Some(TopBarHoverRegion::Session)
        );
    }

    #[test]
    fn dispatch_ui_hover_updates_paste_button_hover() {
        let mut app = make_app();
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 5,
                line_count: 1,
            },
            preview: "hello".into(),
            hovered_button: None,
        });
        let cx = app.ui_context();
        let component = PasteDialogComponent::capture(&app, &cx).unwrap();
        let (x, y, _, _) = component.paste_button;
        let hover = app.dispatch_ui_hover(x + 2.0, y + 2.0);
        assert!(hover.handled);
        assert_eq!(hover.cursor, CursorIcon::Pointer);
        assert_eq!(
            app.core
                .pending_paste
                .as_ref()
                .and_then(|p| p.hovered_button),
            Some(super::super::PasteButton::Paste)
        );
    }
}
