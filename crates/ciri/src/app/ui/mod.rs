pub(super) mod bell_flash;
pub(crate) mod connection_status;
mod context_menu;
mod frame;
mod hints_bar;
pub(super) mod ime_preedit;
mod interaction;
pub(super) mod info_box;
mod overview;
mod palette;
mod paste_dialog;
pub(super) mod search_bar;
mod tab_bar;
mod text_layout;
pub(crate) mod tokens;
mod top_bar;
mod transient;
pub(crate) mod types;

#[cfg(test)]
pub(crate) use frame::chrome_rects;
use frame::UiFrame;
pub(crate) use types::*;

use ciri_render::glyph_cache::GlyphInstance;

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
        // Clear the element arena before this paint pass so all chrome
        // widgets allocate into the freshly reset bump space; entering
        // an `ElementArenaScope` makes `with_element_arena` (called
        // from `AnyElement::new` inside every `Div::child`) target this
        // arena for the duration of `frame.paint`.
        self.ui_arena.borrow_mut().clear();
        let _arena_scope = ciri_ui::ElementArenaScope::enter(&self.ui_arena);
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
        let (cell_w, cell_h) = self.ui_cell_metrics();
        let (viewport_w, viewport_h) = self.command_palette_viewport_size();
        ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            Some(&self.ui_taffy_tree),
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
    use ciri_app::app::CommandPaletteState;
    use ciri_config::config::{CiriConfig, StatusBarPosition, TabBarPosition};
    use winit::window::CursorIcon;

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
