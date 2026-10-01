//! Native dictionary popovers for the GPU terminal view. AppKit callbacks only
//! enqueue a request, leaving grid access outside winit's native callback stack.
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{AnyThread, sel};
use objc2_app_kit::{NSEvent, NSView};
use objc2_foundation::{NSAttributedString, NSPoint, NSString};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

use super::{Command, native, text_selection};
use crate::app::App;

const MAX_QUERY_CHARS: usize = 512;
const MAX_QUERY_BYTES: usize = MAX_QUERY_CHARS * 4;

thread_local! {
    static VIEWS: RefCell<HashMap<usize, WindowId>> = RefCell::new(HashMap::new());
}

extern "C" fn quick_look(this: &AnyObject, _selector: Sel, event: &NSEvent) {
    let key = this as *const AnyObject as usize;
    let id = VIEWS.with(|views| views.borrow().get(&key).copied());
    if let Some(id) = id {
        // SAFETY: this method is only installed on winit's NSView subclass.
        let view = unsafe { &*(this as *const AnyObject).cast::<NSView>() };
        let point = view.convertPoint_fromView(event.locationInWindow(), None);
        native::dispatch(Command::LookUpAt { id, point });
    }
}

pub struct ViewHooks {
    view: Retained<NSView>,
    _zoom: Option<super::restore::ZoomHook>,
    original_class: &'static AnyClass,
    pressure_down: bool,
    last_lookup: Option<Instant>,
}

impl ViewHooks {
    pub fn install(window: &Window) -> Option<Self> {
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit owns a live NSView for this Window. Retain it until the
        // guard restores its class, even if the NSWindow has already closed.
        let view = unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }?;
        let original_class = view.class();
        let class = AnyClass::get(c"LoomTerminalView").unwrap_or_else(|| {
            let mut class = ClassBuilder::new(c"LoomTerminalView", original_class)
                .expect("register terminal view extension");
            super::accessibility::install(&mut class);
            super::text_services::install(&mut class);
            // SAFETY: AppKit's quickLookWithEvent: ABI; no additional ivars.
            unsafe {
                class.add_method(
                    sel!(quickLookWithEvent:),
                    quick_look as extern "C" fn(_, _, _),
                );
            }
            class.register()
        });
        assert_eq!(class.superclass(), Some(original_class));
        assert_eq!(class.instance_size(), original_class.instance_size());
        // SAFETY: same superclass and layout; every winit method is inherited.
        unsafe {
            AnyObject::set_class(&view, class);
        }
        VIEWS.with(|views| {
            views
                .borrow_mut()
                .insert(Retained::as_ptr(&view) as usize, window.id());
        });
        Some(Self {
            view,
            _zoom: native::native_window(window).map(super::restore::ZoomHook::install),
            original_class,
            pressure_down: false,
            last_lookup: None,
        })
    }

    pub fn pressure_changed(&mut self, stage: i64) -> bool {
        let down = stage >= 2;
        let first = down && !self.pressure_down;
        self.pressure_down = down;
        first
    }

    /// AppKit can deliver both pressure and Quick Look for one force click.
    pub fn allow_lookup(&mut self) -> bool {
        let now = Instant::now();
        if self
            .last_lookup
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
        {
            return false;
        }
        self.last_lookup = Some(now);
        true
    }

    pub fn pointer(&self) -> Option<NSPoint> {
        let window = self.view.window()?;
        Some(
            self.view
                .convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None),
        )
    }

    pub fn view(&self) -> &NSView {
        &self.view
    }

    pub fn show(&self, text: &str, point: NSPoint) {
        // Menu-bar invocation can leave the pointer outside the content view.
        let bounds = self.view.bounds();
        let point = NSPoint::new(
            point
                .x
                .clamp(bounds.origin.x, bounds.origin.x + bounds.size.width),
            point
                .y
                .clamp(bounds.origin.y, bounds.origin.y + bounds.size.height),
        );
        let text = NSAttributedString::initWithString(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
        );
        self.view
            .showDefinitionForAttributedString_atPoint(Some(&text), point);
    }
}

impl Drop for ViewHooks {
    fn drop(&mut self) {
        super::text_services::remove(&self.view);
        VIEWS.with(|views| {
            views
                .borrow_mut()
                .remove(&(Retained::as_ptr(&self.view) as usize));
        });
        // SAFETY: undo only this guard's layout-preserving subclass extension.
        unsafe {
            AnyObject::set_class(&self.view, self.original_class);
        }
    }
}

/// Menu and keyboard lookup follow the selected text, even when the mouse is
/// outside the terminal. AppKit expects the first character's baseline in points.
pub fn selection_point(app: &App) -> Option<NSPoint> {
    let selection = app.core.selection.as_ref()?;
    let grid = app.core.pane_grids.get(&selection.pane_id)?;
    let (a, b) = (selection.start, selection.end);
    let (a, b) = if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    };
    let row = a.1.max(grid.viewport_top());
    if row > b.1 {
        return None;
    }
    let viewport_row = grid.buffer_to_viewport_row(row)?;
    let col = if row == a.1 { a.0 } else { 0 };
    let (col, _) = grid.snap_selection_to_wide_chars(row, col, col);
    let (_, rect, _) = app
        .core
        .workspaces
        .visible_tiles_2d(
            app.core.anim_mgr.view_offset_x.value() as f32,
            app.core.anim_mgr.view_offset_y.value() as f32,
        )
        .into_iter()
        .find(|(pane, _, _)| *pane == selection.pane_id)?;
    let (cw, ch) = app.cell_dimensions();
    let baseline = app.glyph_cache.as_ref().map_or(ch, |cache| cache.ascent);
    let inset = app.core.config.appearance.border_width + app.core.config.appearance.padding;
    let x = rect.x + inset + col as f32 * cw;
    let y = rect.y + inset + viewport_row as f32 * ch + baseline;
    let view = &app.core.workspaces.view_size;
    if x < 0.0 || x >= view.width || y < 0.0 || y >= view.height {
        return None;
    }
    Some(NSPoint::new(
        (x + app.content_origin_x()) as f64 / app.dpi_scale,
        (y + app.content_origin_y()) as f64 / app.dpi_scale,
    ))
}

pub fn text_at(app: &App, point: NSPoint, prefer_selection: bool) -> Option<String> {
    if app.modal_captures_keyboard() || app.core.overview.active {
        return None;
    }
    if prefer_selection && let Some(selection) = &app.core.selection {
        let grid = app.core.pane_grids.get(&selection.pane_id)?;
        return bounded_query(text_selection::selected_text(
            grid,
            (selection.start, selection.end),
            MAX_QUERY_BYTES,
        )?);
    }
    // Winit's view is flipped, so both coordinate systems start at top-left.
    let (id, col, row) = app.pixel_to_cell(
        (point.x * app.dpi_scale) as f32,
        (point.y * app.dpi_scale) as f32,
    )?;
    let grid = app.core.pane_grids.get(&id)?;
    if grid.password_input {
        return None;
    }
    let (start, end) = grid.word_bounds_at(col, row)?;
    bounded_query(text_selection::selected_text(
        grid,
        ((start, row), (end, row)),
        MAX_QUERY_BYTES,
    )?)
}

fn bounded_query(text: String) -> Option<String> {
    let text = text.trim();
    // A dictionary query should never hand AppKit megabytes of scrollback.
    (!text.is_empty() && text.chars().count() <= MAX_QUERY_CHARS).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_lookup_anchors_to_first_visible_character_baseline() {
        use loom_config::config::{StatusBarPosition, TabBarPosition};
        use loom_protocol::message::{FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER};
        for scale in [1.0, 2.0] {
            let mut config = loom_config::LoomConfig::default();
            config.statusbar.position = StatusBarPosition::Top;
            config.tabbar.position = TabBarPosition::Left;
            let mut app = App::new(config, "lookup-anchor");
            app.dpi_scale = scale;
            app.core
                .workspaces
                .active_mut()
                .add_column_right(1, loom_layout::column::ColumnWidth::Proportion(1.0));
            let mut grid = crate::grid::ClientPaneGrid::new(8, 4, 0);
            grid.viewport[10].flags = FLAG_WIDE_CHAR.to_le_bytes();
            grid.viewport[11].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
            app.core.pane_grids.insert(1, grid);
            assert!(selection_point(&app).is_none());
            app.core.selection = Some(crate::app::Selection {
                pane_id: 1,
                start: (6, 2),
                end: (3, 1),
                active: false,
            });
            let rect = app.core.workspaces.active().visible_tiles(0.0)[0].1;
            let inset =
                app.core.config.appearance.border_width + app.core.config.appearance.padding;
            let expected = NSPoint::new(
                (rect.x + inset + app.content_origin_x() + 2.0 * 8.0) as f64 / scale,
                (rect.y + inset + app.content_origin_y() + 2.0 * 16.0) as f64 / scale,
            );
            assert_eq!(selection_point(&app), Some(expected));
            app.core.selection.as_mut().unwrap().end = (0, 4);
            app.core.selection.as_mut().unwrap().start = (0, 5);
            assert!(selection_point(&app).is_none());
        }
    }

    #[test]
    fn dictionary_selection_rejects_concealed_wide_cells_and_oversized_graphemes() {
        use loom_protocol::message::{FLAG_HIDDEN, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER};
        let mut app = App::new(loom_config::LoomConfig::default(), "lookup-selection");
        let mut grid = crate::grid::ClientPaneGrid::new(3, 1, 0);
        grid.viewport[0].set_ch('中');
        grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();
        grid.viewport[1].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        grid.viewport[2].set_ch('e');
        grid.grapheme_map.insert(2, "e\u{301}".into());
        app.core.pane_grids.insert(1, grid);
        app.core.selection = Some(crate::app::Selection {
            pane_id: 1,
            // Reversed selection starts on the wide character's trailing cell.
            start: (2, 0),
            end: (1, 0),
            active: false,
        });
        let point = NSPoint::new(0.0, 0.0);
        assert_eq!(text_at(&app, point, true).as_deref(), Some("中e\u{301}"));
        app.core.pane_grids.get_mut(&1).unwrap().viewport[0].flags =
            (FLAG_WIDE_CHAR | FLAG_HIDDEN).to_le_bytes();
        assert!(text_at(&app, point, true).is_none());
        let grid = app.core.pane_grids.get_mut(&1).unwrap();
        grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();
        grid.grapheme_map.insert(2, "e".repeat(MAX_QUERY_BYTES + 1));
        assert!(text_at(&app, point, true).is_none());
        assert_eq!(app.core.selection.as_ref().unwrap().start, (2, 0));
    }

    #[test]
    fn dictionary_pointer_respects_retina_chrome_and_concealed_word_cells() {
        use loom_config::config::{StatusBarPosition, TabBarPosition};
        for scale in [1.0, 2.0] {
            let mut config = loom_config::LoomConfig::default();
            config.statusbar.position = StatusBarPosition::Top;
            config.tabbar.position = TabBarPosition::Left;
            let mut app = App::new(config, "lookup-pointer");
            app.dpi_scale = scale;
            app.core
                .workspaces
                .active_mut()
                .add_column_right(1, loom_layout::column::ColumnWidth::Proportion(1.0));
            let mut grid = crate::grid::ClientPaneGrid::new(5, 1, 0);
            for (cell, ch) in grid.viewport.iter_mut().zip("hello".chars()) {
                cell.set_ch(ch);
            }
            app.core.pane_grids.insert(1, grid);
            let rect = app.core.workspaces.active().visible_tiles(0.0)[0].1;
            let inset =
                app.core.config.appearance.border_width + app.core.config.appearance.padding;
            let point = NSPoint::new(
                (rect.x + inset + app.content_origin_x() + 4.0) as f64 / scale,
                (rect.y + inset + app.content_origin_y() + 8.0) as f64 / scale,
            );
            assert_eq!(text_at(&app, point, false).as_deref(), Some("hello"));
            // Even a hidden cell elsewhere in the same word must not be exported.
            app.core.pane_grids.get_mut(&1).unwrap().viewport[3].flags =
                loom_protocol::message::FLAG_HIDDEN.to_le_bytes();
            assert!(text_at(&app, point, false).is_none());
            assert!(text_at(&app, NSPoint::new(0.0, 0.0), false).is_none());
        }
    }

    #[test]
    fn dictionary_selection_respects_password_and_modal_gates_without_changing_selection() {
        let mut app = App::new(loom_config::LoomConfig::default(), "lookup-test");
        let mut grid = crate::grid::ClientPaneGrid::new(5, 1, 100);
        for (cell, ch) in grid.viewport.iter_mut().zip("hello".chars()) {
            cell.set_ch(ch);
        }
        app.core.pane_grids.insert(1, grid);
        app.core.selection = Some(crate::app::Selection {
            pane_id: 1,
            start: (0, 0),
            end: (4, 0),
            active: false,
        });
        let point = NSPoint::new(0.0, 0.0);
        assert_eq!(text_at(&app, point, true).as_deref(), Some("hello"));
        assert_eq!(app.core.selection.as_ref().unwrap().end, (4, 0));
        app.core.pane_grids.get_mut(&1).unwrap().password_input = true;
        assert!(text_at(&app, point, true).is_none());
        app.core.pane_grids.get_mut(&1).unwrap().password_input = false;
        app.core.settings_panel_visible = true;
        assert!(text_at(&app, point, true).is_none());
        app.core.settings_panel_visible = false;
        app.core.overview.active = true;
        assert!(text_at(&app, point, true).is_none());
    }

    #[test]
    fn dictionary_query_bounds_preserve_unicode_and_reject_empty_or_huge_selections() {
        assert_eq!(
            bounded_query(" 你好👋🏽 \n".into()).as_deref(),
            Some("你好👋🏽")
        );
        assert_eq!(bounded_query("\n\t ".into()), None);
        assert_eq!(bounded_query("a".repeat(513)), None);
        assert!(bounded_query("字".repeat(512)).is_some());
    }
}
