mod bell_flash;
pub(crate) mod connection_status;
mod context_menu;
mod frame;
mod hints_bar;
mod ime_preedit;
pub(super) mod info_box;
mod overview;
mod palette;
mod paste_dialog;
mod search_bar;
mod tab_bar;
mod text_layout;
pub(crate) mod tokens;
mod top_bar;
mod transient;
pub(crate) mod types;

pub(crate) use bell_flash::{BellFlashComponent, BellFlashRect};
#[cfg(test)]
pub(crate) use frame::chrome_rects;
use frame::{UiFrame, UiFrameHover};
pub(crate) use ime_preedit::ImePreeditComponent;
pub(crate) use search_bar::SearchBarComponent;
pub(crate) use transient::TransientOverlayFrame;
pub(crate) use types::*;

use ciri_render::glyph_cache::GlyphInstance;
use winit::window::CursorIcon;

use super::App;

impl App {
    pub(crate) fn build_ui(
        &mut self,
        vw: f32,
        vh: f32,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        // Hover/click dispatch can reach build_ui before the atlas is
        // populated (e.g. mouse events during the initial connection
        // phase before renderer init). Return an empty contribution
        // rather than unwrap-panicking on `glyph_cache`.
        if self.glyph_cache.is_none() {
            return;
        }
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
        let top_bar_layout = self.top_bar_layout(vw, vh, cell_w, cell_h, self.ui_shaper.as_ref());
        self.ensure_active_pane_tab_visible(top_bar_layout.tabs_area_px);
        let baseline = cell_h * self.core.config.statusbar.text_baseline;
        let cx = ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            vw,
            vh,
            cell_w,
            cell_h,
            baseline,
        );
        let cache_key = self.ui_scene_hash(vw, vh, cell_w, cell_h, cx.ui_line_h);
        if self.cached_ui_scene.key == Some(cache_key) {
            glyphs.extend_from_slice(&self.cached_ui_scene.glyphs);
            color_glyphs.extend_from_slice(&self.cached_ui_scene.color_glyphs);
            return;
        }

        let top_bar_h = top_bar_layout.bar_height;
        let hints_bar_h = self.hints_bar_height();
        let frame = UiFrame::capture(self, &cx, top_bar_layout, top_bar_h, hints_bar_h);
        {
            let cached_ui = &mut self.cached_ui_scene;
            cached_ui.key = Some(cache_key);
            cached_ui.glyphs.clear();
            cached_ui.color_glyphs.clear();
            cached_ui.sdf_rects.clear();

            let atlas = self.glyph_cache.as_mut().unwrap();
            let mut scene = UiScene {
                atlas,
                glyphs: &mut cached_ui.glyphs,
                color_glyphs: &mut cached_ui.color_glyphs,
                sdf_rects: &mut cached_ui.sdf_rects,
            };

            frame.paint(&cx, &mut scene);
        }

        glyphs.extend_from_slice(&self.cached_ui_scene.glyphs);
        color_glyphs.extend_from_slice(&self.cached_ui_scene.color_glyphs);
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
        ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            viewport_w,
            viewport_h,
            cell_w,
            cell_h,
            cell_h * self.core.config.statusbar.text_baseline,
        )
    }

    pub(crate) fn dispatch_ui_click(&mut self, mx: f32, my: f32) -> bool {
        let (action, consumed) = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.click(self, mx, my, &cx)
        };
        if let Some(action) = action {
            self.apply_ui_action(action);
        }
        consumed
    }

    /// Route a middle-mouse click. Currently the only middle-click handler
    /// is tab close — browsers and most tab-bearing apps treat MMB on a
    /// tab as "close this tab", so we mirror that. Modal overlays and the
    /// overview deliberately don't react to middle-click: they're focus
    /// surfaces where MMB has no meaning, and routing it there would swallow
    /// the event when the user expects it to pass through to the underlying
    /// tab strip (e.g. clicking through a dismissible tooltip).
    pub(crate) fn dispatch_ui_middle_click(&mut self, mx: f32, my: f32) -> bool {
        let (action, consumed) = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.middle_click(mx, my, &cx)
        };
        if let Some(action) = action {
            self.apply_ui_action(action);
        }
        consumed
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
            UiAction::ClosePaneTab(pane_id) => {
                self.core.hovered_pane_tab = None;
                self.send(ciri_protocol::message::ClientMessage::ClosePane { pane_id });
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
        let hover = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.hover(self, mx, my, &cx)
        };

        match hover {
            UiFrameHover::ContextMenu { hovered } => {
                let prev = self.core.context_menu.hovered_index;
                self.core.context_menu.hovered_index = hovered;
                return UiHoverOutcome {
                    handled: true,
                    cursor: if hovered.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: prev != hovered,
                };
            }
            UiFrameHover::PasteDialog { button } => {
                let prev = self
                    .core
                    .pending_paste
                    .as_ref()
                    .and_then(|p| p.hovered_button);
                if let Some(pending) = &mut self.core.pending_paste {
                    pending.hovered_button = button;
                }
                return UiHoverOutcome {
                    handled: true,
                    cursor: if button.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: prev != button,
                };
            }
            UiFrameHover::Palette { hovered, pointer } => {
                let prev_hovered = self
                    .core
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.hovered_idx);
                if let Some(palette) = &mut self.core.command_palette {
                    palette.hovered_idx = hovered;
                }
                return UiHoverOutcome {
                    handled: true,
                    cursor: if pointer {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: prev_hovered != hovered,
                };
            }
            UiFrameHover::TopBar { region, tab } => {
                let prev_region = self.core.hovered_top_bar_region;
                let prev_tab = self.core.hovered_pane_tab;
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
            UiFrameHover::SideTab { tab } => {
                let prev_tab = self.core.hovered_pane_tab;
                self.core.hovered_pane_tab = tab;
                return UiHoverOutcome {
                    handled: true,
                    cursor: if tab.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: prev_tab != tab,
                };
            }
            UiFrameHover::Overview {
                target,
                action_hover,
            } => {
                let had_top_bar_hover = self.core.hovered_top_bar_region.take().is_some()
                    || self.core.hovered_pane_tab.take().is_some();
                let prev = self.core.overview.hovered_pane;
                let prev_action = self.core.overview_action_hover;
                self.core.overview.hovered_pane = target;
                self.core.overview_action_hover = action_hover;
                return UiHoverOutcome {
                    handled: true,
                    cursor: if target.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: had_top_bar_hover
                        || prev != target
                        || prev_action != action_hover,
                };
            }
            UiFrameHover::None => {
                let had_top_bar_hover = self.core.hovered_top_bar_region.take().is_some()
                    || self.core.hovered_pane_tab.take().is_some();

                UiHoverOutcome {
                    handled: had_top_bar_hover,
                    cursor: CursorIcon::Default,
                    needs_redraw: had_top_bar_hover,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::paste_guard::PasteInfo;
    use crate::app::ui::top_bar::TopBarComponent;
    use crate::app::{ContextMenu, ContextMenuAction, ContextMenuItem, PendingPaste};
    use crate::app::{TopBarHoverRegion, ui::context_menu::ContextMenuComponent};
    use crate::app::{ui::overview::OverviewComponent, ui::paste_dialog::PasteDialogComponent};
    use ciri_app::app::CommandPaletteState;
    use ciri_config::config::{CiriConfig, StatusBarPosition, TabBarPosition};

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn chrome_rects_place_bottom_status_hints_above_top_bar() {
        let mut config = CiriConfig::default();
        config.statusbar.position = StatusBarPosition::Bottom;
        let app = App::new(config, "test-session");

        let rects = chrome_rects(&app, 800.0, 600.0, 24.0, 24.0);

        assert_eq!(rects.hints_bar, UiRect::new(0.0, 552.0, 800.0, 24.0));
        assert_eq!(rects.top_bar, UiRect::new(0.0, 576.0, 800.0, 24.0));
    }

    #[test]
    fn chrome_rects_side_tab_bar_sits_between_horizontal_chrome() {
        let mut config = CiriConfig::default();
        config.statusbar.position = StatusBarPosition::Top;
        config.tabbar.position = TabBarPosition::Left;
        config.tabbar.width = 96.0;
        let app = App::new(config, "test-session");

        let rects = chrome_rects(&app, 800.0, 600.0, 24.0, 24.0);

        assert_eq!(
            rects.side_tab_bar,
            Some(UiRect::new(0.0, 24.0, 96.0, 552.0))
        );
    }

    #[test]
    fn top_bar_session_click_maps_to_open_session_palette() {
        let app = make_app();
        let cx = app.ui_context();
        let layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let component = TopBarComponent::capture(&app, layout, &cx);
        // Session zone is always at x=0 and `SessionLabel` is the first row
        // slot. Use a small positive x to land inside the zone; bar_y is read
        // from the captured layout.
        let action = component.click(4.0, layout.bar_y + 2.0, &cx);
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
            target: super::super::PendingPasteTarget::Terminal,
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
        let layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let hover = app.dispatch_ui_hover(2.0, layout.bar_y + 2.0);
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
            target: super::super::PendingPasteTarget::Terminal,
        });
        let cx = app.ui_context();
        let component = PasteDialogComponent::capture(&app, &cx).unwrap();
        let [x, y, _, _] = component
            .hit_bounds_for_test(&cx, UiPasteDialogHit::Paste)
            .expect("paste button hit bounds");
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
