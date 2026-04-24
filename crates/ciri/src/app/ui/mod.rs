mod bell_flash;
pub(crate) mod connection_status;
mod context_menu;
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
pub(crate) mod types;

pub(crate) use bell_flash::{BellFlashComponent, BellFlashRect};
pub(crate) use connection_status::ConnectionStatusComponent;
pub(crate) use context_menu::ContextMenuComponent;
pub(crate) use hints_bar::HintsBarComponent;
pub(crate) use ime_preedit::ImePreeditComponent;
pub(crate) use info_box::InfoBoxComponent;
pub(crate) use overview::OverviewComponent;
pub(crate) use palette::PaletteComponent;
pub(crate) use paste_dialog::PasteDialogComponent;
pub(crate) use search_bar::SearchBarComponent;
pub(crate) use tab_bar::TabBarComponent;
pub(crate) use top_bar::TopBarComponent;
pub(crate) use types::*;

use ciri_config::config::{StatusBarPosition, TabBarPosition};
use ciri_render::glyph_cache::GlyphInstance;
use winit::window::CursorIcon;

use super::{App, TopBarHoverRegion};

#[derive(Debug, Clone, Copy)]
struct ChromeRects {
    top_bar: UiRect,
    hints_bar: UiRect,
    side_tab_bar: Option<UiRect>,
}

struct UiFrame {
    chrome: ChromeRects,
    top_bar: TopBarComponent,
    hints_bar: HintsBarComponent,
    side_tab_bar: Option<TabBarComponent>,
    overview: OverviewComponent,
    overview_bar: Option<overview::OverviewActionBarData>,
    overview_hover: Option<super::OverviewActionHover>,
    infobox: Option<InfoBoxComponent>,
    palette: Option<PaletteComponent>,
    connection_status: Option<ConnectionStatusComponent>,
    paste_dialog: Option<PasteDialogComponent>,
    context_menu: Option<ContextMenuComponent>,
}

enum UiFrameHover {
    ContextMenu {
        hovered: Option<usize>,
    },
    PasteDialog {
        button: Option<super::PasteButton>,
    },
    Palette {
        hovered: Option<usize>,
        pointer: bool,
    },
    TopBar {
        region: Option<TopBarHoverRegion>,
        tab: Option<u64>,
    },
    SideTab {
        tab: Option<u64>,
    },
    Overview {
        target: Option<(usize, u64)>,
        action_hover: Option<super::OverviewActionHover>,
    },
    None,
}

impl UiFrame {
    fn capture(
        app: &App,
        cx: &UiContext<'_>,
        top_bar_layout: super::top_bar::TopBarLayout,
        top_bar_h: f32,
        hints_bar_h: f32,
    ) -> Self {
        let top_bar = TopBarComponent::capture(app, top_bar_layout, cx);
        let hints_bar = HintsBarComponent::capture(app, cx);
        let side_tab_bar = match cx.config.tabbar.position {
            TabBarPosition::Left | TabBarPosition::Right => Some(TabBarComponent::capture(app, cx)),
            TabBarPosition::Integrated => None,
        };
        let overview_bar = if app.core.overview.active && app.core.overview.hovered_pane.is_some() {
            overview::overview_action_bar_data(app, app.core.overview.hovered_pane)
        } else {
            None
        };

        Self {
            chrome: chrome_rects(app, cx.viewport_w, cx.viewport_h, top_bar_h, hints_bar_h),
            top_bar,
            hints_bar,
            side_tab_bar,
            overview: OverviewComponent::capture(app, cx),
            overview_bar,
            overview_hover: app.core.overview_action_hover,
            infobox: InfoBoxComponent::capture(app, cx),
            palette: PaletteComponent::capture(app, cx),
            connection_status: ConnectionStatusComponent::capture(app, cx),
            paste_dialog: PasteDialogComponent::capture(app, cx),
            context_menu: ContextMenuComponent::capture(app, cx),
        }
    }

    fn capture_current(app: &App, cx: &UiContext<'_>) -> Self {
        let top_bar_layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let top_bar_h = top_bar_layout.bar_height;
        Self::capture(app, cx, top_bar_layout, top_bar_h, app.hints_bar_height())
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        self.top_bar.paint(self.chrome.top_bar, cx, scene);
        self.hints_bar.paint(self.chrome.hints_bar, cx, scene);
        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar) {
            tab_bar.paint(rect, cx, scene);
        }

        // Modal / overlay layers position themselves absolutely and are
        // painted after chrome so they sit on top.
        if let Some(d) = &self.overview_bar {
            overview::paint_overview_action_bar(d, self.overview_hover, cx, scene);
        }
        if let Some(component) = &self.infobox {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.palette {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.connection_status {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.paste_dialog {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.context_menu {
            component.paint(cx, scene);
        }
    }

    fn click(&self, app: &App, mx: f32, my: f32, cx: &UiContext<'_>) -> (Option<UiAction>, bool) {
        // Components are checked in reverse paint order: topmost first.
        if let Some(c) = &self.context_menu {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.paste_dialog {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.palette {
            return (c.click(mx, my, cx), true);
        }

        if self.chrome.top_bar.contains(mx, my) {
            return (self.top_bar.click(mx, my, cx), true);
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            return (tab_bar.hit(rect, mx, my, cx), true);
        }

        if app.core.overview.active {
            let action = match self.overview.hit_test(app, mx, my) {
                UiOverviewHit::Pane(ws_idx, pane_id) => {
                    Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
                }
                UiOverviewHit::FocusPane(ws_idx, pane_id) => {
                    Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
                }
                UiOverviewHit::ClosePane(pane_id) => Some(UiAction::CloseOverviewPane(pane_id)),
                UiOverviewHit::Background => Some(UiAction::StartOverviewDrag),
                UiOverviewHit::None => None,
            };
            return (action, true);
        }

        (None, false)
    }

    fn middle_click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> (Option<UiAction>, bool) {
        if self.chrome.top_bar.contains(mx, my) {
            let action = match self.top_bar.hit_test(mx, my, cx) {
                Some(UiTopBarHit::PaneTab(id)) => Some(UiAction::ClosePaneTab(id)),
                _ => None,
            };
            return (action, true);
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            let action = match tab_bar.hit(rect, mx, my, cx) {
                Some(UiAction::FocusPaneTab(id)) => Some(UiAction::ClosePaneTab(id)),
                _ => None,
            };
            return (action, true);
        }

        (None, false)
    }

    fn hover(&self, app: &App, mx: f32, my: f32, cx: &UiContext<'_>) -> UiFrameHover {
        if let Some(component) = &self.context_menu {
            let hovered = match component.hit_test(mx, my, cx) {
                UiContextMenuHit::Entry(idx) => Some(idx),
                UiContextMenuHit::Menu | UiContextMenuHit::None => None,
            };
            return UiFrameHover::ContextMenu { hovered };
        }

        if let Some(component) = &self.paste_dialog {
            let button = match component.hit_test(mx, my, cx) {
                UiPasteDialogHit::Paste => Some(super::PasteButton::Paste),
                UiPasteDialogHit::Cancel => Some(super::PasteButton::Cancel),
                UiPasteDialogHit::Dialog | UiPasteDialogHit::None => None,
            };
            return UiFrameHover::PasteDialog { button };
        }

        if let Some(component) = &self.palette {
            let (hovered, pointer) = match component.hit_test(mx, my, cx) {
                UiPaletteHit::Entry(entry_idx) => {
                    let hovered = app.core.command_palette.as_ref().and_then(|palette| {
                        palette.filtered.iter().position(|&idx| idx == entry_idx)
                    });
                    (hovered, true)
                }
                UiPaletteHit::Panel => (None, false),
                UiPaletteHit::None => (None, false),
            };
            return UiFrameHover::Palette { hovered, pointer };
        }

        if self.chrome.top_bar.contains(mx, my) {
            let (region, tab) = match self.top_bar.hit_test(mx, my, cx) {
                Some(UiTopBarHit::Session) => (Some(TopBarHoverRegion::Session), None),
                Some(UiTopBarHit::Workspace) => (Some(TopBarHoverRegion::Workspace), None),
                Some(UiTopBarHit::Mode) => (Some(TopBarHoverRegion::Mode), None),
                Some(UiTopBarHit::PaneTab(pane_id)) => (None, Some(pane_id)),
                Some(UiTopBarHit::Background) | None => (None, None),
            };
            return UiFrameHover::TopBar { region, tab };
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            let tab = match tab_bar.hit(rect, mx, my, cx) {
                Some(UiAction::FocusPaneTab(id)) => Some(id),
                _ => None,
            };
            return UiFrameHover::SideTab { tab };
        }

        if app.core.overview.active {
            let hit = self.overview.hit_test(app, mx, my);
            let action_hover = match &hit {
                UiOverviewHit::ClosePane(_) => Some(super::OverviewActionHover::Close),
                UiOverviewHit::FocusPane(_, _) => Some(super::OverviewActionHover::Focus),
                _ => None,
            };
            let target = match hit {
                UiOverviewHit::Pane(ws_idx, pane_id)
                | UiOverviewHit::FocusPane(ws_idx, pane_id) => Some((ws_idx, pane_id)),
                UiOverviewHit::ClosePane(_) => app.core.overview.hovered_pane,
                UiOverviewHit::Background | UiOverviewHit::None => None,
            };
            return UiFrameHover::Overview {
                target,
                action_hover,
            };
        }

        UiFrameHover::None
    }
}

pub(crate) struct TransientOverlayFrame {
    pub(crate) search_bar: Option<SearchBarComponent>,
    pub(crate) bell_flash: Option<BellFlashComponent>,
    pub(crate) ime_preedit: Option<ImePreeditComponent>,
}

impl TransientOverlayFrame {
    pub(crate) fn is_empty(&self) -> bool {
        self.search_bar.is_none() && self.bell_flash.is_none() && self.ime_preedit.is_none()
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if let Some(component) = &self.search_bar {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.bell_flash {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.ime_preedit {
            component.paint(cx, scene);
        }
    }
}

fn chrome_rects(app: &App, vw: f32, vh: f32, top_bar_h: f32, hints_bar_h: f32) -> ChromeRects {
    let top_bar = match app.core.config.statusbar.position {
        StatusBarPosition::Top => UiRect::new(0.0, 0.0, vw, top_bar_h),
        StatusBarPosition::Bottom => UiRect::new(0.0, (vh - top_bar_h).max(0.0), vw, top_bar_h),
    };
    let hints_bar = match app.core.config.statusbar.position {
        StatusBarPosition::Top => UiRect::new(0.0, (vh - hints_bar_h).max(0.0), vw, hints_bar_h),
        StatusBarPosition::Bottom => UiRect::new(
            0.0,
            (vh - top_bar_h - hints_bar_h).max(0.0),
            vw,
            hints_bar_h,
        ),
    };
    let side_tab_bar = match app.core.config.tabbar.position {
        TabBarPosition::Integrated => None,
        TabBarPosition::Left | TabBarPosition::Right => {
            let w = app.core.config.tabbar.width;
            let x = match app.core.config.tabbar.position {
                TabBarPosition::Left => 0.0,
                TabBarPosition::Right => (vw - w).max(0.0),
                TabBarPosition::Integrated => unreachable!(),
            };
            let y = match app.core.config.statusbar.position {
                StatusBarPosition::Top => top_bar_h,
                StatusBarPosition::Bottom => 0.0,
            };
            let h = (vh - top_bar_h - hints_bar_h).max(0.0);
            Some(UiRect::new(x, y, w, h))
        }
    };

    ChromeRects {
        top_bar,
        hints_bar,
        side_tab_bar,
    }
}

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
        let ui_line_h = self
            .ui_shaper
            .as_ref()
            .map(|s| s.borrow().line_height())
            .unwrap_or(cell_h);
        let cache_key = self.ui_scene_hash(vw, vh, cell_w, cell_h, ui_line_h);
        if self.cached_ui_scene.key == Some(cache_key) {
            glyphs.extend_from_slice(&self.cached_ui_scene.glyphs);
            color_glyphs.extend_from_slice(&self.cached_ui_scene.color_glyphs);
            return;
        }
        let cx = UiContext {
            config: &self.core.config,
            theme: &self.cached_resolved_theme,
            viewport_w: vw,
            viewport_h: vh,
            cell_w,
            cell_h,
            baseline,
            ui_line_h,
            ui_shaper: self.ui_shaper.as_ref(),
        };

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
        let ui_line_h = self
            .ui_shaper
            .as_ref()
            .map(|s| s.borrow().line_height())
            .unwrap_or(cell_h);
        UiContext {
            config: &self.core.config,
            theme: &self.cached_resolved_theme,
            viewport_w,
            viewport_h,
            cell_w,
            cell_h,
            baseline: cell_h * self.core.config.statusbar.text_baseline,
            ui_line_h,
            ui_shaper: self.ui_shaper.as_ref(),
        }
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
    use crate::app::{ContextMenu, ContextMenuAction, ContextMenuItem, PendingPaste};
    use ciri_app::app::CommandPaletteState;
    use ciri_config::config::CiriConfig;

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
