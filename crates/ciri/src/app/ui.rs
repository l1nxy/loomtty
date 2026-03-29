use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::rect::Rect;
use winit::window::CursorIcon;

use super::status_bar::emit_status_text;
use super::top_bar::{PaneTabLayout, TopBarLayout};
use super::{App, PaletteToggleLayout, TopBarHoverRegion};

pub(crate) struct UiContext<'a> {
    pub config: &'a ciri_config::config::CiriConfig,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub baseline: f32,
}

pub(crate) struct UiScene<'a> {
    pub atlas: &'a mut GlyphCache,
    pub bg_rects: &'a mut Vec<Rect>,
    pub glyphs: &'a mut Vec<GlyphInstance>,
}

pub(crate) trait UiComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>);
}

pub(crate) struct UiHoverOutcome {
    pub handled: bool,
    pub cursor: CursorIcon,
    pub needs_redraw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UiAction {
    OpenSessionPalette,
    ToggleOverview,
    CycleWorkspace,
    FocusPaneTab(u64),
    ToggleSessionPaletteScope,
    ExecutePaletteEntry(usize),
    ClosePalette,
    ExecuteContextMenuEntry(usize),
    CloseContextMenu,
    ConfirmPaste,
    CancelPaste,
    FocusOverviewPane(usize, u64),
    CloseOverviewPane(u64),
    StartOverviewDrag,
}

pub(crate) enum UiTopBarHit {
    Session,
    Workspace,
    Mode,
    PaneTab(u64),
    Background,
}

pub(crate) enum UiPaletteHit {
    Toggle,
    Entry(usize),
    Panel,
    None,
}

pub(crate) enum UiContextMenuHit {
    Entry(usize),
    Menu,
    None,
}

pub(crate) enum UiPasteDialogHit {
    Paste,
    Cancel,
    Dialog,
    None,
}

pub(crate) enum UiOverviewHit {
    Pane(usize, u64),
    ClosePane(u64),
    FocusPane(usize, u64),
    Background,
    None,
}

struct TopBarComponent {
    layout: TopBarLayout,
    session_text: String,
    workspace_label: String,
    mode_label: String,
    mode_color: [f32; 4],
    pane_tabs: Vec<PaneTabLayout>,
    hovered_region: Option<TopBarHoverRegion>,
    hovered_pane_tab: Option<u64>,
    is_leader: bool,
    is_broadcast: bool,
    is_overview: bool,
    tab_scroll: f32,
    tab_scroll_max: f32,
}

struct PaletteRow {
    entry_idx: usize,
    label: String,
    is_selected: bool,
    is_hovered: bool,
    style: PaletteRowStyle,
}

/// Visual category for a palette row (used for color differentiation).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteRowStyle {
    Action,
    Session,
    RemoteHost,
    RemoteSession,
    SshShell,
    SwitchSlot,
}

struct PaletteComponent {
    layout: super::CommandPaletteLayout,
    toggle: Option<PaletteToggleLayout>,
    query: String,
    rows: Vec<PaletteRow>,
    sessions_show_all: bool,
    total_entries: usize,
    selected_idx: usize,
    show_no_matches: bool,
    loading_text: Option<String>,
    error_text: Option<String>,
}

struct ContextMenuRow {
    label: String,
    enabled: bool,
    hovered: bool,
}

struct ContextMenuComponent {
    x: f32,
    y: f32,
    menu_width: f32,
    menu_height: f32,
    item_height: f32,
    rows: Vec<ContextMenuRow>,
}

struct PasteDialogComponent {
    dx: f32,
    dy: f32,
    dialog_w: f32,
    dialog_h: f32,
    title: String,
    preview: String,
    hovered_button: Option<super::PasteButton>,
    paste_button: (f32, f32, f32, f32),
    cancel_button: (f32, f32, f32, f32),
}

struct OverviewComponent {
    hovered_pane: Option<(usize, u64)>,
}

struct OverviewActionBarData {
    pane_x: f32,
    pane_w: f32,
    bar_y: f32,
    bar_h: f32,
    close_x: f32,
    close_w: f32,
    focus_x: f32,
    focus_w: f32,
}

struct HintsBarComponent {
    bar_y: f32,
    bar_h: f32,
    // Left side: status info
    pane_count: usize,
    active_pane_title: String,
    // Right side: contextual hints (flattened, no group titles)
    hints: Vec<HintItem>,
}

struct HintItem {
    key: String,
    label: String,
}

struct InfoBoxComponent {
    title: String,
    rows: Vec<(String, String)>, // (key_display, description)
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn action_short_label(action: &str) -> &str {
    match action {
        "focus_left" => "left",
        "focus_right" => "right",
        "focus_up" => "up",
        "focus_down" => "down",
        "move_pane_left" => "move ←",
        "move_pane_right" => "move →",
        "new_column_right" => "new pane",
        "new_row_below" | "new_workspace_below" | "split_down" => "split ↓",
        "close_pane" => "close",
        "column_width_decrease" => "shrink",
        "column_width_increase" => "grow",
        "column_width_full" => "full",
        "column_width_one_third" => "1/3",
        "column_width_half" => "1/2",
        "column_width_two_thirds" => "2/3",
        "cycle_preset_width" => "next width",
        "cycle_preset_width_reverse" => "prev width",
        "equalize_adjacent_columns" => "equalize",
        "consume_into_column" => "stack",
        "expel_from_column" => "unstack",
        "toggle_broadcast" => "broadcast",
        "toggle_overview" => "overview",
        "exit_overview" => "exit",
        "toggle_command_palette" => "palette",
        "toggle_lock" => "lock",
        "detach" => "detach",
        "scroll_line_up" => "line ↑",
        "scroll_line_down" => "line ↓",
        "scroll_half_page_up" => "half ↑",
        "scroll_half_page_down" => "half ↓",
        "scroll_page_up" => "page ↑",
        "scroll_page_down" => "page ↓",
        "scroll_top" => "top",
        "scroll_bottom" => "bottom",
        s if s.starts_with("enter_mode:workspace") => "workspace",
        s if s.starts_with("enter_mode:session") => "session",
        s if s.starts_with("enter_mode:resize") => "resize",
        s if s.starts_with("enter_mode:move") => "move",
        s if s.starts_with("enter_mode:scroll") => "scroll",
        s if s.starts_with("enter_mode:") => s.strip_prefix("enter_mode:").unwrap_or(s),
        s if s.starts_with("switch_workspace_") => s.strip_prefix("switch_workspace_").unwrap_or(s),
        other => other,
    }
}

/// Build infobox rows from a bindings map. Merges keys that share the same action.
fn build_infobox_rows(bindings: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    use std::collections::HashMap;
    // Group keys by action
    let mut action_to_keys: HashMap<&str, Vec<&str>> = HashMap::new();
    for (key, action) in bindings {
        action_to_keys
            .entry(action.as_str())
            .or_default()
            .push(key.as_str());
    }
    for keys in action_to_keys.values_mut() {
        keys.sort_by_key(|k| k.len());
    }
    // Sort deterministically: by shortest key length, then alphabetically.
    let mut entries: Vec<_> = action_to_keys.into_iter().collect();
    entries.sort_by(|(_, a_keys), (_, b_keys)| {
        a_keys[0].len().cmp(&b_keys[0].len()).then(a_keys[0].cmp(&b_keys[0]))
    });

    entries
        .into_iter()
        .map(|(action, keys)| {
            let key_display = if keys.len() <= 2 {
                keys.join("/")
            } else {
                keys[..2].join("/")
            };
            (key_display, action_short_label(action).to_string())
        })
        .collect()
}

impl App {
    pub(crate) fn build_ui(
        &mut self,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let cell_w = self.glyph_cache.as_ref().map(|c| c.cell_width).unwrap_or(8.0);
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
        let overview_bar = if self.core.overview.active && self.core.overview.hovered_pane.is_some() {
            self.overview_action_bar_data(&cx)
        } else {
            None
        };
        let overview_hover = self.core.overview_action_hover;

        let atlas = self.glyph_cache.as_mut().unwrap();
        let mut scene = UiScene {
            atlas,
            bg_rects,
            glyphs,
        };

        top_bar.paint(&cx, &mut scene);
        hints_bar.paint(&cx, &mut scene);
        if let Some(d) = &overview_bar {
            Self::paint_overview_action_bar_from_data(d, overview_hover, &cx, &mut scene);
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

    fn overview_action_bar_data(&self, cx: &UiContext<'_>) -> Option<OverviewActionBarData> {
        let (_, hovered_id) = self.core.overview.hovered_pane?;
        let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = self.core.workspaces.all_tiles_2d(vox, voy);
        let (vw, vh) = self.command_palette_viewport_size();
        let center_x = vw / 2.0;
        let center_y = vh / 2.0;
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        for (pane_id, tile_rect, _) in &tiles {
            if *pane_id != hovered_id { continue; }
            let tr = if zoom < zoom_threshold {
                ciri_layout::geometry::Rect::new(
                    center_x + (tile_rect.x - center_x) * zoom,
                    center_y + (tile_rect.y - center_y) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };
            // Need at least enough width for both labels
            let min_w = cx.cell_w * 14.0; // ~7 chars per button minimum
            if tr.w < min_w {
                return None;
            }
            let bar_h = (cx.cell_h * 2.0).max(28.0);
            let bar_y = tr.y + tr.h - bar_h;
            // 50/50 split
            let half_w = tr.w / 2.0;
            return Some(OverviewActionBarData {
                pane_x: tr.x, pane_w: tr.w,
                bar_y, bar_h,
                close_x: tr.x, close_w: half_w,
                focus_x: tr.x + half_w, focus_w: half_w,
            });
        }
        None
    }

    fn paint_overview_action_bar_from_data(
        d: &OverviewActionBarData,
        hover: Option<super::OverviewActionHover>,
        cx: &UiContext<'_>,
        scene: &mut UiScene<'_>,
    ) {
        let accent = ciri_config::theme::ThemeConfig::parse_color(&cx.config.theme.accent);
        let text_y = d.bar_y + (d.bar_h - cx.cell_h) * 0.5;

        // Bar background
        scene.bg_rects.push(Rect {
            x: d.pane_x, y: d.bar_y, w: d.pane_w, h: d.bar_h,
            color: [0.0, 0.0, 0.0, 0.8],
        });
        // Divider
        scene.bg_rects.push(Rect {
            x: d.focus_x, y: d.bar_y + 2.0, w: 1.0, h: d.bar_h - 4.0,
            color: [1.0, 1.0, 1.0, 0.15],
        });

        let close_label = "\u{2715} Close";
        let focus_label = "Focus";
        let close_text_w = close_label.chars().count() as f32 * cx.cell_w;
        let focus_text_w = focus_label.chars().count() as f32 * cx.cell_w;

        // Close button — center text in left half
        if hover == Some(super::OverviewActionHover::Close) {
            scene.bg_rects.push(Rect {
                x: d.close_x, y: d.bar_y, w: d.close_w, h: d.bar_h,
                color: [0.9, 0.2, 0.2, 0.5],
            });
        }
        let close_text_x = d.close_x + (d.close_w - close_text_w) * 0.5;
        emit_status_text(
            scene.atlas, close_label, close_text_x, text_y,
            cx.cell_w, cx.baseline, [1.0, 0.6, 0.6, 1.0], scene.glyphs,
        );

        // Focus button — center text in right half
        if hover == Some(super::OverviewActionHover::Focus) {
            scene.bg_rects.push(Rect {
                x: d.focus_x, y: d.bar_y, w: d.focus_w, h: d.bar_h,
                color: [accent[0], accent[1], accent[2], 0.35],
            });
        }
        let focus_text_x = d.focus_x + (d.focus_w - focus_text_w) * 0.5;
        emit_status_text(
            scene.atlas, focus_label, focus_text_x, text_y,
            cx.cell_w, cx.baseline, [1.0, 1.0, 1.0, 0.9], scene.glyphs,
        );
    }

    fn ui_context(&self) -> UiContext<'_> {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let cell_w = self.glyph_cache.as_ref().map(|c| c.cell_width).unwrap_or(8.0);
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

    pub(crate) fn ui_top_bar_hover(&self, mx: f32, my: f32) -> (Option<TopBarHoverRegion>, Option<u64>) {
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

    fn ui_top_bar_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let component = TopBarComponent::capture(
            self,
            self.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h),
            &cx,
        );
        match component.hit_test(mx, my, &cx) {
            Some(UiTopBarHit::Session) => Some(UiAction::OpenSessionPalette),
            Some(UiTopBarHit::Workspace) => Some(UiAction::CycleWorkspace),
            Some(UiTopBarHit::Mode) => Some(UiAction::ToggleOverview),
            Some(UiTopBarHit::PaneTab(pane_id)) => Some(UiAction::FocusPaneTab(pane_id)),
            Some(UiTopBarHit::Background) | None => None,
        }
    }

    pub(crate) fn ui_palette_hover(&mut self, mx: f32, my: f32) -> (Option<usize>, bool) {
        let cx = self.ui_context();
        let Some(component) = PaletteComponent::capture(self, &cx) else {
            return (None, false);
        };
        match component.hit_test(mx, my) {
            UiPaletteHit::Toggle => (None, true),
            UiPaletteHit::Entry(entry_idx) => {
                let hovered = self.core.command_palette.as_ref().and_then(|palette| {
                    palette
                        .filtered
                        .iter()
                        .position(|&idx| idx == entry_idx)
                });
                (hovered, true)
            }
            UiPaletteHit::Panel => (None, false),
            UiPaletteHit::None => (None, false),
        }
    }

    fn ui_palette_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let Some(component) = PaletteComponent::capture(self, &cx) else {
            return None;
        };
        match component.hit_test(mx, my) {
            UiPaletteHit::Toggle => Some(UiAction::ToggleSessionPaletteScope),
            UiPaletteHit::Entry(entry_idx) => Some(UiAction::ExecutePaletteEntry(entry_idx)),
            UiPaletteHit::Panel => None,
            UiPaletteHit::None => Some(UiAction::ClosePalette),
        }
    }

    pub(crate) fn ui_context_menu_hover(&self, mx: f32, my: f32) -> Option<usize> {
        let cx = self.ui_context();
        let Some(component) = ContextMenuComponent::capture(self, &cx) else {
            return None;
        };
        match component.hit_test(mx, my) {
            UiContextMenuHit::Entry(idx) => Some(idx),
            UiContextMenuHit::Menu | UiContextMenuHit::None => None,
        }
    }

    fn ui_context_menu_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let Some(component) = ContextMenuComponent::capture(self, &cx) else {
            return None;
        };
        match component.hit_test(mx, my) {
            UiContextMenuHit::Entry(idx) => Some(UiAction::ExecuteContextMenuEntry(idx)),
            UiContextMenuHit::Menu => None,
            UiContextMenuHit::None => Some(UiAction::CloseContextMenu),
        }
    }

    pub(crate) fn ui_paste_dialog_hover(&self, mx: f32, my: f32) -> Option<super::PasteButton> {
        let cx = self.ui_context();
        let Some(component) = PasteDialogComponent::capture(self, &cx) else {
            return None;
        };
        match component.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(super::PasteButton::Paste),
            UiPasteDialogHit::Cancel => Some(super::PasteButton::Cancel),
            UiPasteDialogHit::Dialog | UiPasteDialogHit::None => None,
        }
    }

    fn ui_paste_dialog_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let Some(component) = PasteDialogComponent::capture(self, &cx) else {
            return None;
        };
        match component.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(UiAction::ConfirmPaste),
            UiPasteDialogHit::Cancel | UiPasteDialogHit::None => Some(UiAction::CancelPaste),
            UiPasteDialogHit::Dialog => None,
        }
    }

    pub(crate) fn ui_overview_hover(&mut self, mx: f32, my: f32) -> Option<(usize, u64)> {
        let cx = self.ui_context();
        let component = OverviewComponent::capture(self, &cx);
        // Also update action hover state for the action bar highlight
        let hit = component.hit_test(self, mx, my);
        self.core.overview_action_hover = match &hit {
            UiOverviewHit::ClosePane(_) => Some(super::OverviewActionHover::Close),
            UiOverviewHit::FocusPane(_, _) => Some(super::OverviewActionHover::Focus),
            _ => None,
        };
        match hit {
            UiOverviewHit::Pane(ws_idx, pane_id)
            | UiOverviewHit::FocusPane(ws_idx, pane_id) => Some((ws_idx, pane_id)),
            UiOverviewHit::ClosePane(_) => self.core.overview.hovered_pane,
            UiOverviewHit::Background | UiOverviewHit::None => None,
        }
    }

    fn ui_overview_action(&self, mx: f32, my: f32) -> Option<UiAction> {
        let cx = self.ui_context();
        let component = OverviewComponent::capture(self, &cx);
        match component.hit_test(self, mx, my) {
            UiOverviewHit::Pane(ws_idx, pane_id) => Some(UiAction::FocusOverviewPane(ws_idx, pane_id)),
            UiOverviewHit::FocusPane(ws_idx, pane_id) => Some(UiAction::FocusOverviewPane(ws_idx, pane_id)),
            UiOverviewHit::ClosePane(pane_id) => Some(UiAction::CloseOverviewPane(pane_id)),
            UiOverviewHit::Background => Some(UiAction::StartOverviewDrag),
            UiOverviewHit::None => None,
        }
    }

    pub(crate) fn dispatch_ui_click(&mut self, mx: f32, my: f32) -> bool {
        let action = if self.core.pending_paste.is_some() {
            self.ui_paste_dialog_action(mx, my)
        } else if self.core.command_palette.is_some() {
            self.ui_palette_action(mx, my)
        } else if self.core.context_menu.visible {
            self.ui_context_menu_action(mx, my)
        } else if self.hit_test_top_bar(mx, my) {
            self.ui_top_bar_action(mx, my)
        } else if self.core.overview.active {
            self.ui_overview_action(mx, my)
        } else {
            None
        };
        if let Some(action) = action {
            self.apply_ui_action(action);
            true
        } else {
            self.core.pending_paste.is_some()
                || self.core.command_palette.is_some()
                || self.core.context_menu.visible
                || self.hit_test_top_bar(mx, my)
                || self.core.overview.active
        }
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
                    let next_idx = (self.core.workspaces.active_workspace_idx + 1) % workspace_count;
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
            UiAction::ToggleSessionPaletteScope => {
                if let Some(palette) = &mut self.core.command_palette
                    && palette.sessions_only
                {
                    palette.sessions_show_all = !palette.sessions_show_all;
                    palette.entries.clear();
                    palette.filtered.clear();
                    palette.selected_idx = 0;
                    palette.hovered_idx = None;
                }
                self.refresh_session_palette();
            }
            UiAction::ExecutePaletteEntry(entry_idx) => {
                let keep_open = self
                    .core.command_palette
                    .as_ref()
                    .and_then(|p| p.entries.get(entry_idx))
                    .is_some_and(|e| matches!(e.kind, super::PaletteEntryKind::RemoteHost { .. }));
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
            let prev = self.core.pending_paste.as_ref().and_then(|p| p.hovered_button);
            let next = self.ui_paste_dialog_hover(mx, my);
            if let Some(pending) = &mut self.core.pending_paste {
                pending.hovered_button = next;
            }
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() { CursorIcon::Pointer } else { CursorIcon::Default },
                needs_redraw: prev != next,
            };
        }

        if self.core.context_menu.visible {
            let prev = self.core.context_menu.hovered_index;
            let next = self.ui_context_menu_hover(mx, my);
            self.core.context_menu.hovered_index = next;
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() { CursorIcon::Pointer } else { CursorIcon::Default },
                needs_redraw: prev != next,
            };
        }

        if self.core.command_palette.is_some() {
            let prev_hovered = self.core.command_palette.as_ref().and_then(|p| p.hovered_idx);
            let (next_hovered, pointer) = self.ui_palette_hover(mx, my);
            if let Some(palette) = &mut self.core.command_palette {
                palette.hovered_idx = next_hovered;
            }
            return UiHoverOutcome {
                handled: true,
                cursor: if pointer { CursorIcon::Pointer } else { CursorIcon::Default },
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

        let had_top_bar_hover =
            self.core.hovered_top_bar_region.take().is_some() || self.core.hovered_pane_tab.take().is_some();

        if self.core.overview.active {
            let prev = self.core.overview.hovered_pane;
            let next = self.ui_overview_hover(mx, my);
            self.core.overview.hovered_pane = next;
            return UiHoverOutcome {
                handled: true,
                cursor: if next.is_some() { CursorIcon::Pointer } else { CursorIcon::Default },
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

impl TopBarComponent {
    fn capture(app: &App, layout: TopBarLayout, cx: &UiContext<'_>) -> Self {
        let (mode_label, mode_color) = app.current_mode_label();
        let workspace_label = app.workspace_indicator_label();
        let pane_tabs = app.pane_tab_layouts(cx.cell_w, layout.tabs_area_px);
        Self {
            layout,
            session_text: format!(" {}  ", app.core.session_name),
            workspace_label,
            mode_label,
            mode_color,
            pane_tabs,
            hovered_region: app.core.hovered_top_bar_region,
            hovered_pane_tab: app.core.hovered_pane_tab,
            is_leader: app.core.input.is_awaiting_action(),
            is_broadcast: app.core.broadcast_mode,
            is_overview: app.core.overview.active,
            tab_scroll: app.core.pane_tab_scroll,
            tab_scroll_max: app.pane_tab_scroll_max(),
        }
    }

    fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiTopBarHit> {
        if my < self.layout.bar_y || my > self.layout.bar_y + self.layout.bar_height {
            return None;
        }
        if mx >= self.layout.session_x && mx <= self.layout.session_x + self.layout.session_w {
            return Some(UiTopBarHit::Session);
        }
        if self.layout.workspace_w > 0.0
            && mx >= self.layout.workspace_x
            && mx <= self.layout.workspace_x + self.layout.workspace_w
        {
            return Some(UiTopBarHit::Workspace);
        }
        if mx >= self.layout.mode_x && mx <= self.layout.mode_x + self.layout.mode_w {
            return Some(UiTopBarHit::Mode);
        }
        let tabs_start_x = self.layout.session_x + self.layout.session_w;
        let tabs_end_x = tabs_start_x + self.layout.tabs_area_px;
        for tab in &self.pane_tabs {
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            if mx >= visible_left && mx <= visible_right {
                return Some(UiTopBarHit::PaneTab(tab.pane_id));
            }
        }
        let _ = cx;
        Some(UiTopBarHit::Background)
    }
}

impl UiComponent for TopBarComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let padding = if let Some(px) = cx.config.statusbar.height_padding {
            px
        } else {
            cx.cell_h * cx.config.statusbar.padding_ratio
        };
        let bar_height = cx.cell_h + padding;
        let text_y = self.layout.bar_y + padding * 0.5;
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&cx.config.theme.mode_broadcast);

        // Bar background — flat, no per-element pill backgrounds
        scene.bg_rects.push(Rect {
            x: 0.0,
            y: self.layout.bar_y,
            w: cx.viewport_w,
            h: bar_height,
            color: bar_bg,
        });

        // 1px separator between bar and content
        let sep_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - 1.0,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };
        scene.bg_rects.push(Rect {
            x: 0.0, y: sep_y, w: cx.viewport_w, h: 1.0,
            color: [dim[0], dim[1], dim[2], 0.25],
        });

        // Session name — text only, no background
        let session_color = if self.hovered_region == Some(TopBarHoverRegion::Session) { fg } else { dim };
        emit_status_text(
            scene.atlas, &self.session_text, 0.0, text_y,
            cx.cell_w, cx.baseline, session_color, scene.glyphs,
        );

        // Workspace indicator — accent text only, no background
        if !self.workspace_label.is_empty() {
            emit_status_text(
                scene.atlas, &self.workspace_label, self.layout.workspace_x, text_y,
                cx.cell_w, cx.baseline, accent, scene.glyphs,
            );
        }

        // Pane tabs — brightness differentiation + accent bottom indicator
        let indicator_thickness = 1.5_f32;
        let tabs_start_x = self.layout.session_x + self.layout.session_w;
        let tabs_end_x = tabs_start_x + self.layout.tabs_area_px;
        let indicator_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - indicator_thickness,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };

        for tab in &self.pane_tabs {
            let hovered = self.hovered_pane_tab == Some(tab.pane_id);
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }

            // Active tab: accent bottom indicator line
            if tab.active {
                scene.bg_rects.push(Rect {
                    x: visible_left,
                    y: indicator_y,
                    w: visible_w,
                    h: indicator_thickness,
                    color: accent,
                });
            }

            // Text: active → foreground, hovered → foreground, inactive → dim
            let tab_text_color = if tab.active {
                fg
            } else if hovered {
                fg
            } else {
                dim
            };
            if let Some((label, label_x)) =
                clip_tab_label(&tab.label, tab.x, tab.w, cx.cell_w, tabs_start_x, tabs_end_x)
            {
                emit_status_text(
                    scene.atlas, &label, label_x, text_y,
                    cx.cell_w, cx.baseline, tab_text_color, scene.glyphs,
                );
            }
        }

        // Scroll fade (kept — functional, not decorative)
        let fade_w = (cx.cell_w * 3.0).min(self.layout.tabs_area_px * 0.25);
        if fade_w > 0.0 {
            if self.tab_scroll > 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (1.0 - i as f32 / 4.0);
                    let strip_w = fade_w / 4.0 + 1.0;
                    scene.bg_rects.push(Rect {
                        x: tabs_start_x + i as f32 * (fade_w / 4.0),
                        y: self.layout.bar_y,
                        w: strip_w, h: bar_height,
                        color: [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    });
                }
            }
            if self.tab_scroll < self.tab_scroll_max - 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (i as f32 + 1.0) / 4.0;
                    let strip_w = fade_w / 4.0 + 1.0;
                    scene.bg_rects.push(Rect {
                        x: tabs_end_x - fade_w + i as f32 * (fade_w / 4.0),
                        y: self.layout.bar_y,
                        w: strip_w, h: bar_height,
                        color: [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    });
                }
            }
        }

        // Mode label — text only, no pill background
        let mode_str = self.mode_label.as_str();
        let mode_chars = mode_str.chars().count();
        let rx = cx.viewport_w - mode_chars as f32 * cx.cell_w;
        emit_status_text(scene.atlas, mode_str, rx, text_y, cx.cell_w, cx.baseline, self.mode_color, scene.glyphs);

        // Leader / broadcast / overview indicator line
        if self.is_leader || self.is_broadcast || self.is_overview {
            let indicator_h = cx.cell_h * cx.config.statusbar.leader_indicator_ratio;
            let indicator_color = if self.is_broadcast { broadcast_color } else { accent };
            scene.bg_rects.push(Rect {
                x: 0.0,
                y: match cx.config.statusbar.position {
                    StatusBarPosition::Top => self.layout.bar_y + bar_height,
                    StatusBarPosition::Bottom => self.layout.bar_y - indicator_h,
                },
                w: cx.viewport_w,
                h: indicator_h,
                color: indicator_color,
            });
        }
    }
}

impl PaletteComponent {
    fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        let palette = app.core.command_palette.as_ref()?;
        let layout = app.command_palette_layout()?;
        let toggle = app.command_palette_toggle_layout(layout);
        let scroll_offset = app.command_palette_scroll_offset(layout.visible_rows);
        let rows = palette
            .filtered
            .iter()
            .skip(scroll_offset)
            .take(layout.visible_rows)
            .enumerate()
            .map(|(vis_row, filt_idx)| {
                let entry = &palette.entries[*filt_idx];
                let style = match &entry.kind {
                    super::PaletteEntryKind::Action(_) => PaletteRowStyle::Action,
                    super::PaletteEntryKind::SwitchSession(_)
                    | super::PaletteEntryKind::KillSession(_) => PaletteRowStyle::Session,
                    super::PaletteEntryKind::RemoteHost { .. } => PaletteRowStyle::RemoteHost,
                    super::PaletteEntryKind::RemoteSession { .. } => PaletteRowStyle::RemoteSession,
                    super::PaletteEntryKind::SshShell { .. } => PaletteRowStyle::SshShell,
                    super::PaletteEntryKind::SwitchSlot(_) => PaletteRowStyle::SwitchSlot,
                };
                PaletteRow {
                    entry_idx: *filt_idx,
                    label: truncate_label(&entry.label, layout.panel_w, cx.cell_w),
                    is_selected: scroll_offset + vis_row == palette.selected_idx,
                    is_hovered: palette.hovered_idx == Some(scroll_offset + vis_row),
                    style,
                }
            })
            .collect();

        let loading_text = palette
            .remote_loading
            .as_ref()
            .map(|name| format!("Loading sessions from {}...", name));
        let error_text = palette
            .remote_error
            .as_ref()
            .map(|(name, err)| format!("{}: {}", name, err));

        Some(Self {
            layout,
            toggle,
            query: palette.query.clone(),
            rows,
            sessions_show_all: palette.sessions_show_all,
            total_entries: palette.filtered.len(),
            selected_idx: palette.selected_idx,
            show_no_matches: palette.filtered.is_empty() && !palette.query.is_empty(),
            loading_text,
            error_text,
        })
    }

    fn hit_test(&self, mx: f32, my: f32) -> UiPaletteHit {
        if mx < self.layout.panel_x
            || mx > self.layout.panel_x + self.layout.panel_w
            || my < self.layout.panel_y
            || my > self.layout.panel_y + self.layout.panel_h
        {
            return UiPaletteHit::None;
        }
        if let Some(toggle) = self.toggle
            && mx >= toggle.bg_x
            && mx <= toggle.bg_x + toggle.bg_w
            && my >= toggle.bg_y
            && my <= toggle.bg_y + toggle.bg_h
        {
            return UiPaletteHit::Toggle;
        }
        if my < self.layout.sep_y {
            return UiPaletteHit::Panel;
        }
        let vis_row = ((my - self.layout.sep_y) / self.layout.row_h).floor().max(0.0) as usize;
        if vis_row >= self.rows.len() {
            return UiPaletteHit::Panel;
        }
        UiPaletteHit::Entry(self.rows[vis_row].entry_idx)
    }
}

impl UiComponent for PaletteComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg_color = ThemeConfig::parse_color(&cx.config.theme.background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let dim_color = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let text_color = [1.0, 1.0, 1.0, 1.0];

        scene.bg_rects.push(Rect {
            x: 0.0,
            y: 0.0,
            w: cx.viewport_w,
            h: cx.viewport_h,
            color: [0.0, 0.0, 0.0, 0.5],
        });

        let bw = 2.0;
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x - bw,
            y: self.layout.panel_y - bw,
            w: self.layout.panel_w + bw * 2.0,
            h: self.layout.panel_h + bw * 2.0,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.panel_y,
            w: self.layout.panel_w,
            h: self.layout.panel_h,
            color: bg_color,
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.panel_y,
            w: self.layout.panel_w,
            h: self.layout.input_row_h,
            color: [bg_color[0] + 0.05, bg_color[1] + 0.05, bg_color[2] + 0.05, 1.0],
        });

        let input_text = format!("> {}", self.query);
        emit_status_text(
            scene.atlas,
            &input_text,
            self.layout.text_x,
            self.layout.text_y,
            cx.cell_w,
            cx.baseline,
            text_color,
            scene.glyphs,
        );

        if let Some(toggle) = self.toggle {
            let toggle_label = if self.sessions_show_all { " ALL " } else { " ACTIVE " };
            let toggle_bg = if self.sessions_show_all {
                [accent[0], accent[1], accent[2], 0.22]
            } else {
                [accent[0], accent[1], accent[2], 0.12]
            };
            scene.bg_rects.push(Rect {
                x: toggle.bg_x,
                y: toggle.bg_y,
                w: toggle.bg_w,
                h: toggle.bg_h,
                color: toggle_bg,
            });
            emit_status_text(
                scene.atlas,
                toggle_label,
                toggle.label_x,
                self.layout.text_y,
                cx.cell_w,
                cx.baseline,
                text_color,
                scene.glyphs,
            );
        }

        let cursor_x = self.layout.text_x + input_text.len() as f32 * cx.cell_w;
        scene.bg_rects.push(Rect {
            x: cursor_x,
            y: self.layout.text_y,
            w: 2.0,
            h: cx.cell_h,
            color: [1.0, 1.0, 1.0, 0.8],
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.sep_y - 1.0,
            w: self.layout.panel_w,
            h: 1.0,
            color: border_color,
        });

        let selected_bg = [accent[0], accent[1], accent[2], 0.25];
        let hovered_bg = [accent[0], accent[1], accent[2], 0.14];
        // Per-style colors
        let remote_host_color = [accent[0] * 0.8, accent[1] * 1.1, accent[2] * 1.2, 1.0];
        let remote_session_color = [0.6, 0.85, 1.0, 1.0];
        let ssh_color = [1.0, 0.75, 0.4, 1.0];
        let slot_color = [accent[0], accent[1], accent[2], 1.0];

        for (idx, row) in self.rows.iter().enumerate() {
            let row_y = self.layout.sep_y + idx as f32 * self.layout.row_h;
            if row.is_selected {
                scene.bg_rects.push(Rect {
                    x: self.layout.panel_x,
                    y: row_y,
                    w: self.layout.panel_w,
                    h: self.layout.row_h,
                    color: selected_bg,
                });
            } else if row.is_hovered {
                scene.bg_rects.push(Rect {
                    x: self.layout.panel_x,
                    y: row_y,
                    w: self.layout.panel_w,
                    h: self.layout.row_h,
                    color: hovered_bg,
                });
            }
            let row_color = if row.is_selected || row.is_hovered {
                text_color
            } else {
                match row.style {
                    PaletteRowStyle::Action => dim_color,
                    PaletteRowStyle::Session => dim_color,
                    PaletteRowStyle::RemoteHost => remote_host_color,
                    PaletteRowStyle::RemoteSession => remote_session_color,
                    PaletteRowStyle::SshShell => ssh_color,
                    PaletteRowStyle::SwitchSlot => slot_color,
                }
            };
            emit_status_text(
                scene.atlas,
                &row.label,
                self.layout.text_x,
                row_y + 2.0,
                cx.cell_w,
                cx.baseline,
                row_color,
                scene.glyphs,
            );
        }

        if self.total_entries > self.layout.visible_rows {
            let track_w = 4.0;
            let track_x = self.layout.panel_x + self.layout.panel_w - 8.0;
            let track_y = self.layout.sep_y + 2.0;
            let track_h = self.layout.entry_count as f32 * self.layout.row_h - 4.0;
            scene.bg_rects.push(Rect {
                x: track_x,
                y: track_y,
                w: track_w,
                h: track_h.max(0.0),
                color: [border_color[0], border_color[1], border_color[2], 0.20],
            });

            let thumb_h = (track_h * (self.layout.visible_rows as f32 / self.total_entries as f32))
                .max(self.layout.row_h * 0.75);
            let scroll_offset = if self.selected_idx >= self.layout.visible_rows {
                self.selected_idx - self.layout.visible_rows + 1
            } else {
                0
            };
            let thumb_y = track_y
                + (track_h - thumb_h).max(0.0)
                    * (scroll_offset as f32 / (self.total_entries - self.layout.visible_rows) as f32);
            scene.bg_rects.push(Rect {
                x: track_x,
                y: thumb_y,
                w: track_w,
                h: thumb_h,
                color: [accent[0], accent[1], accent[2], 0.65],
            });
        }

        let footer = if self.total_entries > 0 {
            format!("{}/{}", self.selected_idx + 1, self.total_entries)
        } else {
            "0/0".to_string()
        };
        let footer_x = self.layout.panel_x + self.layout.panel_w - (footer.len() as f32 * cx.cell_w) - 12.0;
        let footer_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h - 2.0;
        emit_status_text(
            scene.atlas,
            &footer,
            footer_x,
            footer_y,
            cx.cell_w,
            cx.baseline,
            dim_color,
            scene.glyphs,
        );

        if self.show_no_matches {
            emit_status_text(
                scene.atlas,
                "No matching commands",
                self.layout.text_x,
                self.layout.sep_y + 4.0,
                cx.cell_w,
                cx.baseline,
                dim_color,
                scene.glyphs,
            );
        }

        // Loading indicator
        if let Some(ref loading) = self.loading_text {
            let loading_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h * 2.0 - 4.0;
            emit_status_text(
                scene.atlas,
                loading,
                self.layout.text_x,
                loading_y,
                cx.cell_w,
                cx.baseline,
                [accent[0], accent[1], accent[2], 0.7],
                scene.glyphs,
            );
        }

        // Error message
        if let Some(ref error) = self.error_text {
            let error_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h * 2.0 - 4.0;
            emit_status_text(
                scene.atlas,
                error,
                self.layout.text_x,
                error_y,
                cx.cell_w,
                cx.baseline,
                [1.0, 0.4, 0.3, 0.9],
                scene.glyphs,
            );
        }
    }
}

impl ContextMenuComponent {
    fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.core.context_menu.visible {
            return None;
        }
        let item_height = cx.cell_h * 1.5;
        let padding = 8.0;
        let menu_width = 200.0;
        let menu_height = app.core.context_menu.items.len() as f32 * item_height + padding * 2.0;
        let x = app.core.context_menu.x.min(cx.viewport_w - menu_width);
        let y = app.core.context_menu.y.min(cx.viewport_h - menu_height);
        let rows = app
            .core.context_menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| ContextMenuRow {
                label: item.label.clone(),
                enabled: item.enabled,
                hovered: Some(i) == app.core.context_menu.hovered_index && item.enabled,
            })
            .collect();
        Some(Self {
            x,
            y,
            menu_width,
            menu_height,
            item_height,
            rows,
        })
    }

    fn hit_test(&self, mx: f32, my: f32) -> UiContextMenuHit {
        if mx < self.x || mx > self.x + self.menu_width || my < self.y || my > self.y + self.menu_height {
            return UiContextMenuHit::None;
        }
        let padding = 8.0;
        let relative_y = my - self.y - padding;
        if relative_y < 0.0 {
            return UiContextMenuHit::Menu;
        }
        let index = (relative_y / self.item_height) as usize;
        if index < self.rows.len() {
            UiContextMenuHit::Entry(index)
        } else {
            UiContextMenuHit::Menu
        }
    }
}

impl UiComponent for ContextMenuComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let padding = 8.0;
        let shadow_offset = 3.0;
        scene.bg_rects.push(Rect {
            x: self.x + shadow_offset,
            y: self.y + shadow_offset,
            w: self.menu_width,
            h: self.menu_height,
            color: [0.0, 0.0, 0.0, 0.4],
        });

        let menu_bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let bg_color = [menu_bg[0] * 0.9, menu_bg[1] * 0.9, menu_bg[2] * 0.9, 1.0];
        scene.bg_rects.push(Rect {
            x: self.x,
            y: self.y,
            w: self.menu_width,
            h: self.menu_height,
            color: bg_color,
        });

        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let bw = 1.0;
        scene.bg_rects.push(Rect { x: self.x, y: self.y, w: self.menu_width, h: bw, color: border_color });
        scene.bg_rects.push(Rect { x: self.x, y: self.y + self.menu_height - bw, w: self.menu_width, h: bw, color: border_color });
        scene.bg_rects.push(Rect { x: self.x, y: self.y, w: bw, h: self.menu_height, color: border_color });
        scene.bg_rects.push(Rect { x: self.x + self.menu_width - bw, y: self.y, w: bw, h: self.menu_height, color: border_color });

        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let fg_color = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim_base = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let dim_color = [dim_base[0], dim_base[1], dim_base[2], 0.75];
        for (i, row) in self.rows.iter().enumerate() {
            let iy = self.y + padding + i as f32 * self.item_height;
            if row.hovered {
                scene.bg_rects.push(Rect {
                    x: self.x + bw,
                    y: iy,
                    w: self.menu_width - bw * 2.0,
                    h: self.item_height,
                    color: [accent[0], accent[1], accent[2], 0.12],
                });
            }
            let text_y = iy + (self.item_height - cx.cell_h) * 0.5;
            emit_status_text(
                scene.atlas,
                &row.label,
                self.x + padding,
                text_y,
                cx.cell_w,
                cx.baseline,
                if row.enabled { fg_color } else { dim_color },
                scene.glyphs,
            );
        }
    }
}

impl InfoBoxComponent {
    fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        // Don't show infobox when palette or paste dialog is active
        if app.core.command_palette.is_some() || app.core.pending_paste.is_some() {
            return None;
        }

        // Show when: named mode, leader awaiting, OR help panel toggled
        let (title, bindings) = if let Some(mode_name) = app.core.input.current_mode_name() {
            let bindings = app.core.config.keys.modes.get(mode_name)?;
            (mode_name.to_uppercase(), bindings.clone())
        } else if app.core.input.is_awaiting_action() {
            ("LEADER".to_string(), app.core.config.keys.bindings.clone())
        } else {
            return None;
        };

        let mut rows = build_infobox_rows(&bindings);
        rows.push(("esc".to_string(), "exit".to_string()));

        let padding = cx.cell_w;
        let row_h = cx.cell_h * 1.3;
        let key_col_chars = rows.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
        let val_col_chars = rows.iter().map(|(_, v)| v.chars().count()).max().unwrap_or(0);
        let title_chars = title.chars().count() + 4; // " TITLE " + border padding
        let content_chars = key_col_chars + 3 + val_col_chars; // key + "   " + val
        let box_chars = content_chars.max(title_chars);
        let w = box_chars as f32 * cx.cell_w + padding * 2.0;
        let h = rows.len() as f32 * row_h + padding * 2.0 + cx.cell_h; // extra row for title

        // Position: bottom-right, above the hints bar
        let hints_bar_h = app.hints_bar_height();
        let status_bar_h = app.status_bar_height();
        let margin = 8.0;
        let x = cx.viewport_w - w - margin;
        let bottom_chrome = match cx.config.statusbar.position {
            StatusBarPosition::Top => hints_bar_h,
            StatusBarPosition::Bottom => status_bar_h + hints_bar_h,
        };
        let y = cx.viewport_h - h - bottom_chrome - margin;

        Some(Self { title, rows, x, y, w, h })
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let fg = [1.0f32, 1.0, 1.0, 0.9];
        let dim = [1.0f32, 1.0, 1.0, 0.5];
        let padding = cx.cell_w;
        let row_h = cx.cell_h * 1.3;
        let bw = 1.0f32;

        // Shadow
        scene.bg_rects.push(Rect {
            x: self.x + 3.0, y: self.y + 3.0, w: self.w, h: self.h,
            color: [0.0, 0.0, 0.0, 0.4],
        });
        // Background
        scene.bg_rects.push(Rect {
            x: self.x, y: self.y, w: self.w, h: self.h,
            color: [bg[0] * 0.85, bg[1] * 0.85, bg[2] * 0.85, 0.97],
        });
        // Border
        scene.bg_rects.push(Rect { x: self.x, y: self.y, w: self.w, h: bw, color: accent });
        scene.bg_rects.push(Rect { x: self.x, y: self.y + self.h - bw, w: self.w, h: bw, color: accent });
        scene.bg_rects.push(Rect { x: self.x, y: self.y, w: bw, h: self.h, color: accent });
        scene.bg_rects.push(Rect { x: self.x + self.w - bw, y: self.y, w: bw, h: self.h, color: accent });

        // Title bar background
        let title_h = cx.cell_h + 2.0;
        scene.bg_rects.push(Rect {
            x: self.x + bw, y: self.y + bw, w: self.w - bw * 2.0, h: title_h,
            color: [accent[0], accent[1], accent[2], 0.2],
        });

        // Title text
        let title_text = format!(" {} ", self.title);
        let title_y = self.y + bw + (title_h - cx.cell_h) * 0.5;
        emit_status_text(
            scene.atlas, &title_text, self.x + padding, title_y,
            cx.cell_w, cx.baseline, accent, scene.glyphs,
        );

        // Key column width
        let key_col_chars = self.rows.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);

        // Rows
        let content_y = self.y + bw + title_h + 4.0;
        for (i, (key, desc)) in self.rows.iter().enumerate() {
            let ry = content_y + i as f32 * row_h;
            let text_y = ry + (row_h - cx.cell_h) * 0.5;

            // Key (accent color, right-aligned within key column)
            let key_chars = key.chars().count();
            let key_offset = (key_col_chars - key_chars) as f32 * cx.cell_w;
            emit_status_text(
                scene.atlas, key, self.x + padding + key_offset, text_y,
                cx.cell_w, cx.baseline, accent, scene.glyphs,
            );

            // Description (dim color)
            let desc_x = self.x + padding + (key_col_chars as f32 + 2.0) * cx.cell_w;
            emit_status_text(
                scene.atlas, desc, desc_x, text_y,
                cx.cell_w, cx.baseline,
                if key == "esc" { dim } else { fg },
                scene.glyphs,
            );
        }
    }
}

impl HintsBarComponent {
    fn capture(app: &App, cx: &UiContext<'_>) -> Self {
        let bar_h = app.hints_bar_height();
        let bar_y = app.hints_bar_y(cx.viewport_h);

        // Left side: pane count + active pane title
        let ws = app.core.workspaces.active();
        let pane_count = ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>();
        let active_pane_title = ws
            .active_pane_id()
            .and_then(|id| app.core.pane_grids.get(&id))
            .map(|g| g.title.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();

        // Right side: pick only the 3-4 most relevant hints for current state
        let hints = Self::pick_hints(app);

        Self { bar_y, bar_h, pane_count, active_pane_title, hints }
    }

    /// Pick a small number of contextual hints (max ~4) for the current input state.
    fn pick_hints(app: &App) -> Vec<HintItem> {
        let h = |key: &str, label: &str| HintItem { key: key.into(), label: label.into() };

        // Helper: find the first key bound to an action
        let find_key = |action: &str, bindings: &std::collections::HashMap<String, String>| -> Option<String> {
            bindings.iter()
                .find(|(_, v)| v.as_str() == action)
                .map(|(k, _)| k.clone())
        };

        if app.core.input.is_locked() {
            // Locked: just show how to unlock
            let key = find_key("toggle_lock", &app.core.config.keys.direct_bindings)
                .or_else(|| find_key("toggle_lock", &app.core.config.keys.bindings))
                .unwrap_or_else(|| "g".into());
            return vec![h(&key, "unlock")];
        }

        if app.core.overview.active {
            return vec![
                h("hjkl", "move"),
                h("enter", "select"),
                h("esc", "exit"),
            ];
        }

        if let Some(mode_name) = app.core.input.current_mode_name() {
            let mut hints = Vec::new();
            if let Some(mode_bindings) = app.core.config.keys.modes.get(mode_name) {
                // Show top 3 bindings from the mode, sorted by key simplicity
                let mut entries: Vec<_> = mode_bindings.iter().collect();
                entries.sort_by_key(|(k, _)| k.len());
                for (key, action) in entries.into_iter().take(3) {
                    hints.push(h(key, action_short_label(action)));
                }
            }
            hints.push(h("esc", "exit"));
            return hints;
        }

        if app.core.input.is_awaiting_action() {
            // Leader awaiting: show the most common actions
            let bindings = &app.core.config.keys.bindings;
            let mut hints = Vec::new();
            for (action, label) in [
                ("new_column_right", "new"),
                ("close_pane", "close"),
                ("toggle_overview", "overview"),
                ("toggle_command_palette", "palette"),
            ] {
                if let Some(key) = find_key(action, bindings) {
                    hints.push(h(&key, label));
                }
            }
            return hints;
        }

        // Idle/Normal: show direct bindings (including promoted alt+key in sticky mode)
        let mut hints = Vec::new();
        // In promoted bare-modifier mode, leader is disabled — don't show it
        if !app.core.input.uses_bare_modifier_promotion() {
            hints.push(h(&app.core.config.keys.leader, "leader"));
        }
        // Pick a few useful direct bindings to show
        let direct = &app.core.input.direct_keybinds;
        for (action, label) in [
            ("toggle_help", "help"),
            ("toggle_lock", "lock"),
        ] {
            if let Some(key_display) = direct.find_key_for_action(action) {
                hints.push(h(&key_display, label));
            }
        }
        hints
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);

        // Bar background
        scene.bg_rects.push(Rect {
            x: 0.0, y: self.bar_y, w: cx.viewport_w, h: self.bar_h,
            color: bar_bg,
        });

        // 1px separator at top edge
        scene.bg_rects.push(Rect {
            x: 0.0, y: self.bar_y, w: cx.viewport_w, h: 1.0,
            color: [dim[0], dim[1], dim[2], 0.25],
        });

        let text_y = self.bar_y + (self.bar_h - cx.cell_h) * 0.5;
        let padding = cx.cell_w;

        // ── Left side: status info ──
        let mut x = padding;

        // Accent dot indicator
        emit_status_text(
            scene.atlas, "\u{25CF}", x, text_y,
            cx.cell_w, cx.baseline, accent, scene.glyphs,
        );
        x += cx.cell_w * 2.0;

        // Pane count
        let pane_text = if self.pane_count == 1 {
            "1 pane".to_string()
        } else {
            format!("{} panes", self.pane_count)
        };
        emit_status_text(
            scene.atlas, &pane_text, x, text_y,
            cx.cell_w, cx.baseline, dim, scene.glyphs,
        );
        x += pane_text.chars().count() as f32 * cx.cell_w;

        // Separator dot + active pane title
        if !self.active_pane_title.is_empty() {
            let sep = " \u{00B7} ";
            emit_status_text(
                scene.atlas, sep, x, text_y,
                cx.cell_w, cx.baseline, dim, scene.glyphs,
            );
            x += sep.chars().count() as f32 * cx.cell_w;

            let max_title_chars = 24;
            let title: String = self.active_pane_title.chars().take(max_title_chars).collect();
            emit_status_text(
                scene.atlas, &title, x, text_y,
                cx.cell_w, cx.baseline, fg, scene.glyphs,
            );
        }

        // ── Right side: hint keys (right-aligned) ──
        // Pre-calculate total width of hints to right-align
        let hint_spacing = cx.cell_w * 2.0;
        let mut total_hints_w = 0.0_f32;
        for (i, item) in self.hints.iter().enumerate() {
            if i > 0 {
                total_hints_w += hint_spacing;
            }
            total_hints_w += (item.key.chars().count() + 1 + item.label.chars().count()) as f32 * cx.cell_w;
        }

        let max_hints_w = cx.viewport_w * 0.6;
        let mut rx = cx.viewport_w - padding - total_hints_w.min(max_hints_w);

        for (i, item) in self.hints.iter().enumerate() {
            if rx > cx.viewport_w - padding {
                break;
            }
            if i > 0 {
                rx += hint_spacing;
            }

            // Key in accent
            emit_status_text(
                scene.atlas, &item.key, rx, text_y,
                cx.cell_w, cx.baseline, accent, scene.glyphs,
            );
            rx += item.key.chars().count() as f32 * cx.cell_w;

            // Label in dim
            let label_text = format!(" {}", item.label);
            emit_status_text(
                scene.atlas, &label_text, rx, text_y,
                cx.cell_w, cx.baseline, dim, scene.glyphs,
            );
            rx += label_text.chars().count() as f32 * cx.cell_w;
        }
    }
}

impl PasteDialogComponent {
    fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        let pending = app.core.pending_paste.as_ref()?;
        let dialog_w = cx.viewport_w * 0.6;
        let dialog_h = cx.viewport_h * 0.4;
        let dx = (cx.viewport_w - dialog_w) / 2.0;
        let dy = (cx.viewport_h - dialog_h) / 2.0;
        let title = format!(
            "Are you sure you want to paste {} ({} lines)?",
            super::paste_guard::format_size(pending.info.size),
            pending.info.line_count
        );
        let max_chars = ((dialog_w - 32.0) / cx.cell_w) as usize;
        let preview = if pending.preview.len() > max_chars {
            format!(
                "{}...",
                &pending.preview[..pending.preview.floor_char_boundary(max_chars.saturating_sub(3))]
            )
        } else {
            pending.preview.clone()
        };
        let btn_w = 100.0;
        let btn_h = cx.cell_h + 12.0;
        let btn_y = dy + dialog_h - 16.0 - btn_h;
        let paste_x = dx + dialog_w / 2.0 - btn_w - 16.0;
        let cancel_x = dx + dialog_w / 2.0 + 16.0;

        Some(Self {
            dx,
            dy,
            dialog_w,
            dialog_h,
            title,
            preview,
            hovered_button: pending.hovered_button,
            paste_button: (paste_x, btn_y, btn_w, btn_h),
            cancel_button: (cancel_x, btn_y, btn_w, btn_h),
        })
    }

    fn hit_test(&self, mx: f32, my: f32) -> UiPasteDialogHit {
        let (paste_x, paste_y, paste_w, paste_h) = self.paste_button;
        if mx >= paste_x && mx <= paste_x + paste_w && my >= paste_y && my <= paste_y + paste_h {
            return UiPasteDialogHit::Paste;
        }
        let (cancel_x, cancel_y, cancel_w, cancel_h) = self.cancel_button;
        if mx >= cancel_x && mx <= cancel_x + cancel_w && my >= cancel_y && my <= cancel_y + cancel_h {
            return UiPasteDialogHit::Cancel;
        }
        if mx >= self.dx && mx <= self.dx + self.dialog_w && my >= self.dy && my <= self.dy + self.dialog_h {
            return UiPasteDialogHit::Dialog;
        }
        UiPasteDialogHit::None
    }
}

/// Action bar button rects: (close_x, close_w, focus_x, focus_w, bar_y, bar_h)
struct OverviewActionBar {
    close_x: f32,
    close_w: f32,
    focus_x: f32,
    focus_w: f32,
    bar_y: f32,
    bar_h: f32,
    pane_x: f32,
    pane_w: f32,
}

impl OverviewComponent {
    fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
        Self {
            hovered_pane: app.core.overview.hovered_pane,
        }
    }

    fn hit_test(&self, app: &App, mx: f32, my: f32) -> UiOverviewHit {
        if !app.core.overview.active {
            return UiOverviewHit::None;
        }
        // Check action bar on hovered pane first
        if let Some((ws_idx, hovered_id)) = self.hovered_pane {
            if let Some(bar) = self.action_bar_layout(app) {
                if mx >= bar.pane_x
                    && mx <= bar.pane_x + bar.pane_w
                    && my >= bar.bar_y
                    && my <= bar.bar_y + bar.bar_h
                {
                    if mx >= bar.close_x && mx < bar.close_x + bar.close_w {
                        return UiOverviewHit::ClosePane(hovered_id);
                    }
                    if mx >= bar.focus_x && mx < bar.focus_x + bar.focus_w {
                        return UiOverviewHit::FocusPane(ws_idx, hovered_id);
                    }
                    // Clicked on bar but not a button — don't fall through to pane click
                    return UiOverviewHit::Background;
                }
            }
        }
        if let Some((ws_idx, pane_id)) = app.hit_test_overview(mx, my) {
            UiOverviewHit::Pane(ws_idx, pane_id)
        } else {
            UiOverviewHit::Background
        }
    }

    fn action_bar_layout(&self, app: &App) -> Option<OverviewActionBar> {
        let (_, hovered_id) = self.hovered_pane?;
        let zoom = app.core.anim_mgr.overview_zoom.value() as f32;
        let vox = app.core.anim_mgr.view_offset_x.value() as f32;
        let voy = app.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = app.core.workspaces.all_tiles_2d(vox, voy);
        let (vw, vh) = app.command_palette_viewport_size();
        let center_x = vw / 2.0;
        let center_y = vh / 2.0;
        let zoom_threshold = app.core.config.animation.zoom_threshold;
        let cell_w = app.glyph_cache.as_ref().map(|c| c.cell_width).unwrap_or(8.0);
        let cell_h = app.glyph_cache.as_ref().map(|c| c.cell_height).unwrap_or(16.0);

        for (pane_id, tile_rect, _) in &tiles {
            if *pane_id != hovered_id {
                continue;
            }
            let tr = if zoom < zoom_threshold {
                ciri_layout::geometry::Rect::new(
                    center_x + (tile_rect.x - center_x) * zoom,
                    center_y + (tile_rect.y - center_y) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };
            let min_w = cell_w * 14.0;
            if tr.w < min_w {
                return None;
            }
            let bar_h = (cell_h * 2.0).max(28.0);
            let bar_y = tr.y + tr.h - bar_h;
            let half_w = tr.w / 2.0;

            return Some(OverviewActionBar {
                close_x: tr.x, close_w: half_w,
                focus_x: tr.x + half_w, focus_w: half_w,
                bar_y, bar_h,
                pane_x: tr.x, pane_w: tr.w,
            });
        }
        None
    }
}

impl UiComponent for PasteDialogComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        scene.bg_rects.push(Rect {
            x: 0.0,
            y: 0.0,
            w: cx.viewport_w,
            h: cx.viewport_h,
            color: [0.0, 0.0, 0.0, 0.5],
        });
        scene.bg_rects.push(Rect {
            x: self.dx,
            y: self.dy,
            w: self.dialog_w,
            h: self.dialog_h,
            color: [0.12, 0.12, 0.15, 1.0],
        });

        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let border = 1.0;
        scene.bg_rects.push(Rect { x: self.dx, y: self.dy, w: self.dialog_w, h: border, color: border_color });
        scene.bg_rects.push(Rect { x: self.dx, y: self.dy + self.dialog_h - border, w: self.dialog_w, h: border, color: border_color });
        scene.bg_rects.push(Rect { x: self.dx, y: self.dy, w: border, h: self.dialog_h, color: border_color });
        scene.bg_rects.push(Rect { x: self.dx + self.dialog_w - border, y: self.dy, w: border, h: self.dialog_h, color: border_color });

        let text_x = self.dx + 16.0;
        let mut text_y = self.dy + 16.0;
        emit_status_text(scene.atlas, &self.title, text_x, text_y, cx.cell_w, cx.baseline, [0.9, 0.9, 0.9, 1.0], scene.glyphs);
        text_y += cx.cell_h + 12.0;
        emit_status_text(scene.atlas, "Preview:", text_x, text_y, cx.cell_w, cx.baseline, [0.6, 0.6, 0.6, 1.0], scene.glyphs);
        text_y += cx.cell_h + 4.0;
        scene.bg_rects.push(Rect {
            x: text_x - 4.0,
            y: text_y - 2.0,
            w: self.dialog_w - 24.0,
            h: cx.cell_h + 4.0,
            color: [0.08, 0.08, 0.1, 1.0],
        });
        emit_status_text(scene.atlas, &self.preview, text_x, text_y, cx.cell_w, cx.baseline, [0.6, 0.6, 0.6, 1.0], scene.glyphs);

        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let (paste_x, btn_y, btn_w, btn_h) = self.paste_button;
        let (cancel_x, _, _, _) = self.cancel_button;
        let paste_bg = if self.hovered_button == Some(super::PasteButton::Paste) {
            [accent[0], accent[1], accent[2], 0.8]
        } else {
            [accent[0], accent[1], accent[2], 0.5]
        };
        scene.bg_rects.push(Rect { x: paste_x, y: btn_y, w: btn_w, h: btn_h, color: paste_bg });
        emit_status_text(
            scene.atlas,
            "Paste",
            paste_x + (btn_w - cx.cell_w * 5.0) / 2.0,
            btn_y + (btn_h - cx.cell_h) / 2.0,
            cx.cell_w,
            cx.baseline,
            [1.0, 1.0, 1.0, 1.0],
            scene.glyphs,
        );

        let cancel_bg = if self.hovered_button == Some(super::PasteButton::Cancel) {
            [0.4, 0.4, 0.4, 0.8]
        } else {
            [0.3, 0.3, 0.3, 0.5]
        };
        scene.bg_rects.push(Rect { x: cancel_x, y: btn_y, w: btn_w, h: btn_h, color: cancel_bg });
        emit_status_text(
            scene.atlas,
            "Cancel",
            cancel_x + (btn_w - cx.cell_w * 6.0) / 2.0,
            btn_y + (btn_h - cx.cell_h) / 2.0,
            cx.cell_w,
            cx.baseline,
            [0.9, 0.9, 0.9, 1.0],
            scene.glyphs,
        );
    }
}

fn clip_tab_label(
    label: &str,
    tab_x: f32,
    tab_w: f32,
    cw: f32,
    tabs_start_x: f32,
    tabs_end_x: f32,
) -> Option<(String, f32)> {
    let visible_left = tab_x.max(tabs_start_x);
    let visible_right = (tab_x + tab_w).min(tabs_end_x);
    if visible_right <= visible_left {
        return None;
    }
    let skip_left = ((visible_left - tab_x) / cw).floor().max(0.0) as usize;
    let visible_chars = ((visible_right - visible_left) / cw).floor().max(0.0) as usize;
    let clipped = label.chars().skip(skip_left).take(visible_chars).collect::<String>();
    if clipped.is_empty() {
        None
    } else {
        Some((clipped, visible_left))
    }
}

fn truncate_label(label: &str, panel_w: f32, cw: f32) -> String {
    let max_chars = ((panel_w - 16.0) / cw).floor().max(1.0) as usize;
    if label.len() > max_chars {
        format!("{}...", &label[..label.floor_char_boundary(max_chars.saturating_sub(3))])
    } else {
        label.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{CommandPaletteState, ContextMenu, ContextMenuAction, ContextMenuItem, PendingPaste};
    use crate::app::paste_guard::PasteInfo;
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn top_bar_session_click_maps_to_open_session_palette() {
        let app = make_app();
        let cx = app.ui_context();
        let layout = app.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h);
        let action = app.ui_top_bar_action(layout.session_x + 4.0, layout.bar_y + 2.0);
        assert_eq!(action, Some(UiAction::OpenSessionPalette));
    }

    #[test]
    fn palette_toggle_click_maps_to_scope_toggle() {
        let mut app = make_app();
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: true,
            sessions_show_all: false,
            remote_loading: None,
            remote_error: None,
        });
        let layout = app.command_palette_layout().unwrap();
        let toggle = app.command_palette_toggle_layout(layout).unwrap();
        let action = app.ui_palette_action(toggle.bg_x + 2.0, toggle.bg_y + 2.0);
        assert_eq!(action, Some(UiAction::ToggleSessionPaletteScope));
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
        let action = app.ui_context_menu_action(60.0, 65.0);
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
        let action = app.ui_paste_dialog_action(0.0, 0.0);
        assert_eq!(action, Some(UiAction::CancelPaste));
    }

    #[test]
    fn overview_background_click_maps_to_start_drag() {
        let mut app = make_app();
        app.core.overview.active = true;
        let action = app.ui_overview_action(10.0, 10.0);
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
            sessions_show_all: true,
            remote_loading: None,
            remote_error: None,
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
        assert_eq!(app.core.hovered_top_bar_region, Some(TopBarHoverRegion::Session));
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
            app.core.pending_paste.as_ref().and_then(|p| p.hovered_button),
            Some(super::super::PasteButton::Paste)
        );
    }
}
