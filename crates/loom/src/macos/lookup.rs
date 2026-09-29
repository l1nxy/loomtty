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

use super::{Command, native};
use crate::app::App;

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

pub fn text_at(app: &App, point: NSPoint, prefer_selection: bool) -> Option<String> {
    if app.modal_captures_keyboard() || app.core.overview.active {
        return None;
    }
    if prefer_selection && let Some(selection) = &app.core.selection {
        let grid = app.core.pane_grids.get(&selection.pane_id)?;
        if grid.password_input {
            return None;
        }
        return bounded_query(app.extract_selected_text()?);
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
    bounded_query(grid.text_in_range((start, row), (end, row)))
}

fn bounded_query(text: String) -> Option<String> {
    let text = text.trim();
    // A dictionary query should never hand AppKit megabytes of scrollback.
    (!text.is_empty() && text.chars().count() <= 512).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
