pub(super) mod bell_flash;
pub(crate) mod connection_status;
pub(crate) mod context_menu;
pub(super) mod debug_panel;
mod frame;
mod hints_bar;
pub(super) mod ime_preedit;
pub(super) mod info_box;
mod interaction;
pub(crate) mod overview;
mod palette;
mod paste_dialog;
pub(super) mod search_bar;
pub(crate) mod settings_panel;
mod tab_bar;
mod text_layout;
pub(crate) mod tokens;
mod top_bar;
mod transient;
pub(crate) mod types;

use frame::UiFrame;
#[cfg(test)]
pub(crate) use frame::chrome_rects;
pub(crate) use types::*;

use loom_render::glyph_cache::GlyphInstance;

use super::App;

impl App {
    pub(super) fn ui_cell_metrics(&self) -> (f32, f32) {
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
        (cell_w, cell_h)
    }

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
        let (cell_w, cell_h) = self.ui_cell_metrics();
        let top_bar_layout = self.top_bar_layout(vw, vh, cell_w, cell_h, self.ui_shaper.as_ref());
        self.ensure_active_pane_tab_visible(top_bar_layout.tabs_area_px);
        let baseline = cell_h * self.core.config.statusbar.text_baseline;
        let cx = ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            Some(&self.ui_taffy_tree),
            self.last_mouse_pos.map(|(x, y)| [x, y]),
            self.effective_active_hit_id(),
            Some(&self.ui_states),
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
        let mut frame = UiFrame::capture(self, &cx, top_bar_layout, top_bar_h, hints_bar_h);
        // Clear the element arena before this paint pass so all chrome
        // widgets allocate into the freshly reset bump space; entering
        // an `ElementArenaScope` makes `with_element_arena` (called
        // from `AnyElement::new` inside every `Div::child`) target this
        // arena for the duration of `frame.paint`.
        self.ui_arena.borrow_mut().clear();
        let _arena_scope = loom_ui::ElementArenaScope::enter(&self.ui_arena);
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

            // Two-phase paint to materialise the Base / Overlay
            // z-layers as contiguous ranges in each primitive Vec.
            // After the base pass returns, the current Vec lengths
            // are the layer split points; the overlay pass appends
            // onto the same Vecs so the final layout is
            // `[base | overlay]` per stream — the renderer draws the
            // base half (rects then glyphs) before issuing the overlay
            // half so popup rects can occlude base glyphs.
            frame.paint_base(&cx, &mut scene);
            cached_ui.base_glyph_end = cached_ui.glyphs.len();
            cached_ui.base_color_glyph_end = cached_ui.color_glyphs.len();
            cached_ui.base_sdf_end = cached_ui.sdf_rects.len();

            let mut scene = UiScene {
                atlas: self.glyph_cache.as_mut().unwrap(),
                glyphs: &mut cached_ui.glyphs,
                color_glyphs: &mut cached_ui.color_glyphs,
                sdf_rects: &mut cached_ui.sdf_rects,
            };
            frame.paint_overlay(&cx, &mut scene);
        }

        glyphs.extend_from_slice(&self.cached_ui_scene.glyphs);
        color_glyphs.extend_from_slice(&self.cached_ui_scene.color_glyphs);
    }

    pub(crate) fn ui_context(&self) -> UiContext<'_> {
        let (cell_w, cell_h) = self.ui_cell_metrics();
        let (viewport_w, viewport_h) = self.command_palette_viewport_size();
        ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            Some(&self.ui_taffy_tree),
            self.last_mouse_pos.map(|(x, y)| [x, y]),
            self.effective_active_hit_id(),
            Some(&self.ui_states),
            viewport_w,
            viewport_h,
            cell_w,
            cell_h,
            cell_h * self.core.config.statusbar.text_baseline,
        )
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
    use loom_app::app::CommandPaletteState;
    use loom_config::config::{LoomConfig, StatusBarPosition, TabBarPosition};
    use winit::window::CursorIcon;

    fn make_app() -> App {
        App::new(LoomConfig::default(), "test-session")
    }

    #[test]
    fn chrome_rects_place_bottom_status_hints_above_top_bar() {
        let mut config = LoomConfig::default();
        config.statusbar.position = StatusBarPosition::Bottom;
        let app = App::new(config, "test-session");

        let rects = chrome_rects(&app, 800.0, 600.0, 24.0, 24.0);

        assert_eq!(rects.hints_bar, UiRect::new(0.0, 552.0, 800.0, 24.0));
        assert_eq!(rects.top_bar, UiRect::new(0.0, 576.0, 800.0, 24.0));
    }

    #[test]
    fn chrome_rects_side_tab_bar_sits_between_horizontal_chrome() {
        let mut config = LoomConfig::default();
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
        // After Step 28 the field is gone; assert via the derive
        // helper that backs the chrome cache key.
        app.last_mouse_pos = Some((2.0, layout.bar_y + 2.0));
        assert_eq!(
            app.current_top_bar_region_hover(),
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
        // Hover is now derived from cursor + dialog geometry (no stored
        // field); the assertion is that the helper sees the cursor over
        // the Paste button after the hover dispatch.
        app.last_mouse_pos = Some((x + 2.0, y + 2.0));
        assert_eq!(
            app.current_paste_dialog_hover(),
            Some(super::super::PasteButton::Paste)
        );
    }

    /// Mouse-up always clears `App::active_hit_id`, regardless of how
    /// it was captured at press time. The `.active()` refinement on
    /// any chrome reads the field via `cx.is_active(hit_id)` and
    /// stops matching as soon as it goes back to `None`.
    #[test]
    fn mouse_release_clears_active_hit_id() {
        use winit::event::MouseButton;

        let mut app = make_app();
        // Simulate a press having captured some chrome hit_id. The
        // exact value doesn't matter for the lifecycle test — only
        // that release transitions `Some(_) → None`.
        app.active_hit_id = Some(super::top_bar::pane_tab_hit_id(42));

        app.handle_mouse_released(MouseButton::Left);
        assert_eq!(
            app.active_hit_id, None,
            "mouse-up must drop any captured press-state hit_id",
        );
    }

    /// `capture_active_press_hit_id` is the seam between mouse-down
    /// routing and `App::active_hit_id`. It returns `None` for clicks
    /// that don't land on press-friendly chrome (pane content, mode
    /// indicator, modal-only widgets) — guards against a phantom
    /// press state appearing for non-clickable / dismiss-on-click
    /// regions.
    #[test]
    fn capture_active_press_hit_id_returns_none_outside_press_friendly_chrome() {
        let app = make_app();
        // Far below the top bar — pane content area.
        assert_eq!(app.capture_active_press_hit_id(100.0, 300.0), None);
        // Top-bar mode region (right edge). Mode does have a click
        // action (ToggleOverview) but no `.hover()` / `.active()`
        // styling, so it'\''s deliberately excluded from the press-
        // friendly set — adding press feedback alone (no hover) would
        // feel inconsistent with the rest of the bar.
        let cx = app.ui_context();
        let layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let mode_x = cx.viewport_w - 4.0;
        assert_eq!(
            app.capture_active_press_hit_id(mode_x, layout.bar_y + 2.0),
            None,
            "mode region has no hover/press styling ⇒ press should not capture a hit_id",
        );
    }

    /// Top-bar session label and workspace indicator both opt into
    /// `.active()` press feedback — Step 39 extension. Click coords
    /// inside their slots return `Some(HIT_SESSION)` /
    /// `Some(HIT_WORKSPACE)` so the next paint can apply the press
    /// tint.
    #[test]
    fn capture_active_press_hit_id_matches_session_and_workspace() {
        use crate::app::ui::top_bar::TopBarComponent;

        let app = make_app();
        let cx = app.ui_context();
        let layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let bar_y = layout.bar_y + layout.bar_height * 0.5;
        let bar_rect =
            super::types::UiRect::new(0.0, layout.bar_y, cx.viewport_w, layout.bar_height);
        let component = TopBarComponent::capture(&app, layout, &cx);
        let slots = component.row_slots(bar_rect, &cx);

        // Session is the leftmost slot — well-defined for the default
        // App. Click in its centre.
        let session_x = slots.session.x + slots.session.w * 0.5;
        assert_eq!(
            app.capture_active_press_hit_id(session_x, bar_y),
            Some(super::top_bar::HIT_SESSION),
            "session label should be press-friendly",
        );

        // Workspace slot is only non-empty when there's a label —
        // default App has the workspace indicator visible.
        if slots.workspace.w > 0.0 {
            let workspace_x = slots.workspace.x + slots.workspace.w * 0.5;
            assert_eq!(
                app.capture_active_press_hit_id(workspace_x, bar_y),
                Some(super::top_bar::HIT_WORKSPACE),
                "workspace indicator should be press-friendly",
            );
        }
    }

    /// Mouse-down outside any press-friendly chrome leaves
    /// `active_hit_id` unset — clicking on terminal content should not
    /// flip on a phantom press state.
    #[test]
    fn mouse_press_outside_chrome_leaves_active_hit_id_unset() {
        use winit::event::MouseButton;

        let mut app = make_app();
        app.handle_mouse_pressed(MouseButton::Left, 100.0, 300.0);
        assert_eq!(
            app.active_hit_id, None,
            "press outside press-friendly chrome should not set active_hit_id",
        );
    }

    // ── Settings panel ────────────────────────────────────────────────

    /// `Action::ToggleSettings` is a true toggle, AND it closes every
    /// other modal-ish overlay so the panel cleanly owns input focus.
    /// Catches anyone reverting the round-1 fix that added the search /
    /// context_menu close calls (palette was always closed).
    #[test]
    fn toggle_settings_closes_other_overlays_and_flips_visibility() {
        use loom_input::action::Action;

        let mut app = make_app();
        // Seed the kinds of overlays that should be force-closed.
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 10.0,
            y: 10.0,
            target_pane_id: None,
            items: vec![],
        };
        // paste_dialog sits ahead of settings_panel in `UiFrame::click`
        // (Base-tier modal precedence), so leaving it open would
        // intercept every click intended for the settings panel. Pin
        // the close-others contract for it the same way the palette
        // and overview tests do.
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "x".into(),
                size: 1,
                line_count: 1,
            },
            preview: "x".into(),
            target: super::super::PendingPasteTarget::Terminal,
        });

        app.handle_action(Action::ToggleSettings);
        assert!(app.core.settings_panel_visible, "first toggle opens panel");
        assert!(
            app.core.command_palette.is_none(),
            "palette must be force-closed when settings opens",
        );
        assert!(
            !app.core.context_menu.visible,
            "context_menu must be force-closed when settings opens",
        );
        assert!(
            app.core.pending_paste.is_none(),
            "paste dialog must close when settings opens — it would \
             otherwise intercept clicks intended for the panel",
        );

        // Open the theme dropdown as a submodal of the panel —
        // `ContextMenu(OverSettings)` declares the parent-child
        // relationship. Closing the panel via toggle MUST also tear
        // down its submodal; otherwise the dropdown floats orphaned
        // and the next keypress hits `dismiss_context_menu_on_keypress`
        // silently. Pinned because the toggle-off branch had a bare
        // `settings_panel_visible = false` that bypassed the gate.
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 0.0,
            y: 0.0,
            target_pane_id: None,
            items: vec![],
        };

        app.handle_action(Action::ToggleSettings);
        assert!(!app.core.settings_panel_visible, "second toggle closes");
        assert!(
            !app.core.context_menu.visible,
            "toggle-off must close the theme-dropdown submodal with its parent panel",
        );
    }

    /// Closing a parent modal (palette, search, settings) via the
    /// toggle action must tear down its submodal — otherwise the
    /// child overlay floats orphaned with no underlying state. This
    /// pins the contract that ALL parent-close paths route through
    /// `enter_modal_close_peers(ModalKind::None)`, not bare field
    /// writes. Bug-shape: paste-confirm dialog opened OVER the
    /// palette query, then user toggles palette closed; the dialog
    /// stays visible confirming a paste with nowhere to land.
    #[test]
    fn toggle_palette_closed_kills_paste_overlay_submodal() {
        use loom_app::app::{CommandPaletteState, PendingPasteTarget};
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "x".into(),
                size: 1,
                line_count: 1,
            },
            preview: "x".into(),
            target: PendingPasteTarget::CommandPalette,
        });

        app.handle_action(Action::ToggleCommandPalette);

        assert!(app.core.command_palette.is_none(), "palette closed");
        assert!(
            app.core.pending_paste.is_none(),
            "paste-over-palette submodal must die with parent palette",
        );
    }

    /// Selecting an `Action(ToggleSettings)` entry from the palette
    /// must leave the settings panel OPEN. The execute flow is:
    /// (1) `execute_palette_entry` → `handle_action(ToggleSettings)`
    /// → ToggleSettings opens settings via the gate, which closes
    /// palette as a peer.
    /// (2) Back in `execute_palette_selection`, `!keep_open` is true.
    /// Without the `command_palette.is_some()` guard, the `None` gate
    /// fires here and nukes the freshly-opened settings panel —
    /// settings flashes open then immediately closed, user sees nothing.
    /// Pinned because the gate-everywhere defensive sweep introduced
    /// this regression and a prior critic round caught it.
    #[test]
    fn execute_palette_action_entry_does_not_double_close_new_modal() {
        use loom_app::app::{CommandPaletteState, PaletteEntry, PaletteEntryKind};
        use loom_input::action::Action;

        let mut app = make_app();
        // Seed a palette with a single Action(ToggleSettings) entry,
        // already filtered + selected so `execute_palette_selection`
        // picks it.
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: vec![PaletteEntry::new(
                "Settings…",
                PaletteEntryKind::Action(Action::ToggleSettings),
            )],
            filtered: vec![0],
            selected_idx: 0,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });

        app.handle_action(Action::PaletteConfirm);

        assert!(
            app.core.settings_panel_visible,
            "settings panel must stay open after palette executes \
             ToggleSettings — the redundant `None` gate after the \
             action would otherwise close it",
        );
        assert!(
            app.core.command_palette.is_none(),
            "palette closed (cleared by ToggleSettings's own gate)",
        );
    }

    /// Mouse-click counterpart of
    /// `execute_palette_action_entry_does_not_double_close_new_modal`.
    /// Clicking a palette entry routes through `apply_ui_action` ->
    /// `UiAction::ExecutePaletteEntry(idx)`, which has the same
    /// `command_palette.is_some()` guard as the keyboard path. A
    /// prior critic round caught that the keyboard test alone left
    /// the mouse path unpinned — this test closes the gap. Same
    /// regression shape: without the guard, selecting "Settings…"
    /// via mouse flashes the panel open then immediately closed.
    #[test]
    fn ui_execute_palette_entry_does_not_double_close_new_modal() {
        use loom_app::app::{CommandPaletteState, PaletteEntry, PaletteEntryKind};
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: vec![PaletteEntry::new(
                "Settings…",
                PaletteEntryKind::Action(Action::ToggleSettings),
            )],
            filtered: vec![0],
            selected_idx: 0,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });

        app.apply_ui_action(UiAction::ExecutePaletteEntry(0));

        assert!(
            app.core.settings_panel_visible,
            "settings panel must stay open after mouse-click executes \
             ToggleSettings — same guard shape as the keyboard path",
        );
        assert!(
            app.core.command_palette.is_none(),
            "palette closed (cleared by ToggleSettings's own gate)",
        );
    }

    /// `close_search_restore_scroll` writes the pre-search scroll
    /// offset back to the pane grid. Every prior search-related test
    /// uses `original_scroll_offset: 0` against a default-zero grid,
    /// so the restore-write path was never exercised: deleting it
    /// would leave the test suite green. Pin the actual mutation
    /// here — seed a non-default scroll on the grid, route through
    /// the gate via `CloseSearch`, assert the original offset is
    /// written back. The comments in `App::toggle_overview` /
    /// `App::enter_modal_close_peers` call this the critical reason
    /// the App-level wrapper exists; pin it with a real assertion.
    #[test]
    fn close_search_restores_pre_search_scroll_offset() {
        use loom_app::app::SearchState;
        use loom_input::action::Action;

        let mut app = make_app();
        let pane_id: u64 = 7;
        // Seed a grid scrolled into history. The user is currently
        // viewing match position `scroll_offset = 50`; before the
        // search was opened, they were at `original_scroll_offset = 5`.
        let mut grid = crate::grid::ClientPaneGrid::new(80, 24, 1000);
        grid.scroll_offset = 50;
        app.core.pane_grids.insert(pane_id, grid);
        app.core.search_state = Some(SearchState {
            query: "abc".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id,
            original_scroll_offset: 5,
        });

        app.handle_action(Action::CloseSearch);

        assert!(
            app.core.search_state.is_none(),
            "CloseSearch tears down search_state",
        );
        let restored = app
            .core
            .pane_grids
            .get(&pane_id)
            .expect("grid still present")
            .scroll_offset;
        assert_eq!(
            restored, 5,
            "pre-search scroll offset (5) must be restored to the grid; \
             without this the user is stranded at the last match position (50)",
        );
    }

    /// Same shape as `toggle_palette_closed_kills_paste_overlay_submodal`
    /// but for search + paste-over-search. `Action::CloseSearch`
    /// must route through the gate so the paste dialog overlaying
    /// the search bar dies with it.
    #[test]
    fn close_search_kills_paste_overlay_submodal() {
        use loom_app::app::{PendingPasteTarget, SearchState};
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.search_state = Some(SearchState {
            query: "abc".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 0,
            original_scroll_offset: 0,
        });
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "x".into(),
                size: 1,
                line_count: 1,
            },
            preview: "x".into(),
            target: PendingPasteTarget::Search,
        });

        app.handle_action(Action::CloseSearch);

        assert!(app.core.search_state.is_none(), "search closed");
        assert!(
            app.core.pending_paste.is_none(),
            "paste-over-search submodal must die with parent search",
        );
    }

    /// `ToggleSessionPalette` close path mirrors `ToggleCommandPalette`
    /// (both clear `command_palette` via `gate(None)`), but the close
    /// branch had no dedicated test pinning the submodal-teardown
    /// invariant. Without this, a future refactor that diverges the
    /// two close branches could orphan a `PendingPaste(CommandPalette)`
    /// submodal on `ToggleSessionPalette` while keeping
    /// `ToggleCommandPalette` correct, and the test suite wouldn't
    /// catch it.
    #[test]
    fn toggle_session_palette_closed_kills_paste_overlay_submodal() {
        use loom_app::app::{CommandPaletteState, PendingPasteTarget};
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            sessions_only: true, // session-palette flavor
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "x".into(),
                size: 1,
                line_count: 1,
            },
            preview: "x".into(),
            target: PendingPasteTarget::CommandPalette,
        });

        app.handle_action(Action::ToggleSessionPalette);

        assert!(app.core.command_palette.is_none(), "session palette closed");
        assert!(
            app.core.pending_paste.is_none(),
            "paste-over-palette submodal must die with the parent session palette",
        );
    }

    /// `ToggleCommandPalette` (and `ToggleSessionPalette`) is the
    /// inverse direction of the previous test — opening a palette must
    /// close any Base-tier modal underneath, otherwise `UiFrame::click`
    /// dispatches mouse events to the visually-occluded modal sitting
    /// behind the palette.
    #[test]
    fn toggle_command_palette_closes_base_modals() {
        use loom_app::app::SearchState;
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.settings_panel_visible = true;
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 0.0,
            y: 0.0,
            target_pane_id: None,
            items: vec![],
        };
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 5,
                line_count: 1,
            },
            preview: "hello".into(),
            target: super::super::PendingPasteTarget::Terminal,
        });
        app.core.search_state = Some(SearchState {
            query: "abc".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 0,
            original_scroll_offset: 0,
        });

        app.handle_action(Action::ToggleCommandPalette);

        assert!(
            app.core.command_palette.is_some(),
            "ToggleCommandPalette opens the palette",
        );
        assert!(
            !app.core.settings_panel_visible,
            "settings panel must close when palette opens",
        );
        assert!(
            !app.core.context_menu.visible,
            "context_menu must close when palette opens",
        );
        assert!(
            app.core.pending_paste.is_none(),
            "paste dialog must close when palette opens (otherwise palette \
             would route clicks past the dialog and lock the user out)",
        );
        assert!(
            app.core.search_state.is_none(),
            "search bar must close when palette opens",
        );
    }

    /// Entering overview must dismiss every modal-ish overlay,
    /// otherwise the settings panel's full-viewport backdrop locks
    /// the user out: `UiFrame::click` / `hover` walk
    /// `settings_panel` ahead of the `overview.active` branch, so
    /// every mouse event is consumed by the panel until the user
    /// closes it with Esc.
    #[test]
    fn toggle_overview_closes_base_modals() {
        use loom_app::app::SearchState;
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.settings_panel_visible = true;
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 0.0,
            y: 0.0,
            target_pane_id: None,
            items: vec![],
        };
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "x".into(),
                size: 1,
                line_count: 1,
            },
            preview: "x".into(),
            target: super::super::PendingPasteTarget::Terminal,
        });
        // Search lives on the transient layer (not modal-gated), so
        // overview entry must clear it explicitly or both widgets stay
        // visible AND keyboard-routable simultaneously.
        app.core.search_state = Some(SearchState {
            query: "abc".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 0,
            original_scroll_offset: 0,
        });

        app.handle_action(Action::ToggleOverview);

        assert!(app.core.overview.active);
        assert!(!app.core.settings_panel_visible);
        assert!(!app.core.context_menu.visible);
        assert!(app.core.pending_paste.is_none());
        assert!(app.core.search_state.is_none());
    }

    /// Mirror of `toggle_command_palette_closes_base_modals` for the
    /// session-palette path. Both actions land in
    /// `App::open_session_palette` / `open_command_palette` which
    /// share the `enter_modal_close_peers` gate; this test guards
    /// against future refactors that diverge the two entry points.
    #[test]
    fn toggle_session_palette_closes_base_modals() {
        use loom_app::app::SearchState;
        use loom_input::action::Action;

        let mut app = make_app();
        app.core.settings_panel_visible = true;
        app.core.context_menu = ContextMenu {
            visible: true,
            x: 0.0,
            y: 0.0,
            target_pane_id: None,
            items: vec![],
        };
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 5,
                line_count: 1,
            },
            preview: "hello".into(),
            target: super::super::PendingPasteTarget::Terminal,
        });
        app.core.search_state = Some(SearchState {
            query: "abc".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 0,
            original_scroll_offset: 0,
        });

        app.handle_action(Action::ToggleSessionPalette);

        assert!(app.core.command_palette.is_some());
        assert!(!app.core.settings_panel_visible);
        assert!(!app.core.context_menu.visible);
        assert!(app.core.pending_paste.is_none());
        assert!(app.core.search_state.is_none());
    }

    // Stepper clamp behaviour now lives in the schema module
    // (`schema::tests::nudge_clamps_at_bounds`) since the dispatcher is
    // a thin wrapper. The integration shape — that pressing the
    // dispatcher's button at the floor doesn't underflow — is covered
    // there for every Float field; no need to re-prove it per UiAction
    // variant.

    /// `SettingsPanelComponent::capture` returns `None` while the panel
    /// is hidden — gates the rest of the chrome paint pipeline.
    #[test]
    fn settings_panel_capture_hidden_returns_none() {
        use crate::app::ui::settings_panel::SettingsPanelComponent;

        let app = make_app();
        let cx = app.ui_context();
        assert!(
            SettingsPanelComponent::capture(&app, &cx).is_none(),
            "panel must be invisible until ToggleSettings fires",
        );
    }

    /// Outside-click on the settings backdrop maps to `CloseSettings`.
    /// Pin the dismissal contract so reordering hit_ids can't silently
    /// turn the backdrop into a no-op.
    #[test]
    fn settings_outside_click_maps_to_close() {
        use crate::app::ui::settings_panel::SettingsPanelComponent;

        let mut app = make_app();
        app.core.settings_panel_visible = true;
        let cx = app.ui_context();
        let component =
            SettingsPanelComponent::capture(&app, &cx).expect("panel visible after toggle");
        // (0, 0) is reliably outside the centred panel for any
        // non-trivial viewport.
        let action = component.click(0.0, 0.0, &cx);
        assert_eq!(action, Some(UiAction::CloseSettings));
    }

    /// Click on the panel body (not on any interactive child) returns
    /// `SettingsNoOp` — the click is absorbed and does NOT dismiss
    /// settings. Pins the `Dialog` no-op contract; without this guard
    /// a future hit_id reordering could silently turn panel-body
    /// clicks into dismissals.
    #[test]
    fn settings_dialog_body_click_is_no_op() {
        use crate::app::ui::settings_panel::SettingsPanelComponent;

        let mut app = make_app();
        app.core.settings_panel_visible = true;
        let cx = app.ui_context();
        let component =
            SettingsPanelComponent::capture(&app, &cx).expect("panel visible after toggle");
        // Probe several "likely panel body" points (region between
        // sidebar and right edge, mid-vertical). At least one must
        // land on the body and produce SettingsNoOp.
        let cx_w = cx.viewport_w;
        let cx_h = cx.viewport_h;
        let candidates: &[(f32, f32)] = &[
            (cx_w * 0.7, cx_h * 0.85),
            (cx_w * 0.7, cx_h * 0.82),
            (cx_w * 0.6, cx_h * 0.85),
        ];
        let any_dialog_hit = candidates
            .iter()
            .any(|&(mx, my)| component.click(mx, my, &cx) == Some(UiAction::SettingsNoOp));
        assert!(
            any_dialog_hit,
            "at least one panel-body candidate must absorb the click as SettingsNoOp",
        );
    }
}
