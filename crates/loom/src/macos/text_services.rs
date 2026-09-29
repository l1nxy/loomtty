//! Requestor side of macOS text Services. Callbacks use cached selection data
//! and enqueue returned text; they never borrow the live Rust application.
use std::cell::RefCell;
use std::collections::HashMap;

use crossbeam_channel::Sender;
use loom_protocol::message::{ClientMessage, FLAG_HIDDEN, FLAG_WIDE_CHAR_SPACER};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ClassBuilder, Sel};
use objc2::{AnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSPasteboard, NSPasteboardTypeString, NSServicesMenuRequestor, NSView,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use winit::window::WindowId;

use crate::app::App;
use crate::grid::ClientPaneGrid;

const MAX_TEXT_BYTES: usize = 1024 * 1024;
type SelectionRange = ((u16, usize), (u16, usize));

#[derive(Clone, Debug)]
struct Target {
    window: WindowId,
    pane: u64,
    session: String,
    sender: Sender<ClientMessage>,
}
impl Target {
    fn same(&self, other: &Self) -> bool {
        self.window == other.window
            && self.pane == other.pane
            && self.session == other.session
            && self.sender.same_channel(&other.sender)
    }
    fn matches(&self, app: &App) -> bool {
        app.window.as_ref().is_some_and(|w| w.id() == self.window)
            && app.core.workspaces.active().active_pane_id() == Some(self.pane)
            && app.core.session_name == self.session
            && app
                .core
                .server_tx
                .as_ref()
                .is_some_and(|tx| tx.same_channel(&self.sender))
            && eligible(app)
    }
}

#[derive(Clone, Debug)]
pub(super) struct Request {
    target: Target,
    text: String,
}
struct Entry {
    target: Target,
    stamp: Option<(u64, SelectionRange)>,
    selected: Option<String>,
    requestor: Retained<LoomTextServiceRequestor>,
}
#[derive(Default)]
struct State {
    next_key: u64,
    views: HashMap<usize, u64>,
    entries: HashMap<u64, Entry>,
}
impl State {
    fn remove(&mut self, view: usize) {
        if let Some(key) = self.views.remove(&view) {
            self.entries.remove(&key);
        }
    }
    fn entry(&mut self, view: usize, target: Target) -> &mut Entry {
        if self
            .views
            .get(&view)
            .and_then(|key| self.entries.get(key))
            .is_some_and(|entry| !entry.target.same(&target))
        {
            self.remove(view);
        }
        let key = *self.views.entry(view).or_insert_with(|| {
            self.next_key += 1;
            self.next_key
        });
        self.entries.entry(key).or_insert_with(|| Entry {
            target,
            stamp: None,
            selected: None,
            requestor: LoomTextServiceRequestor::new(key),
        })
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }

// AppKit retains the returned requestor during a service invocation. A separate
// immutable identity prevents a late result from following this NSView to a new
// pane, even if the Services menu is validated again while the service runs.
define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = u64]
    struct LoomTextServiceRequestor;
    unsafe impl NSObjectProtocol for LoomTextServiceRequestor {}
    unsafe impl NSServicesMenuRequestor for LoomTextServiceRequestor {
        #[unsafe(method(writeSelectionToPasteboard:types:))]
        fn write(&self, pasteboard: &NSPasteboard, types: &NSArray<NSString>) -> bool {
            write_selection(*self.ivars(), pasteboard, types)
        }
        #[unsafe(method(readSelectionFromPasteboard:))]
        fn read(&self, pasteboard: &NSPasteboard) -> bool {
            read_selection(*self.ivars(), pasteboard)
        }
    }
);
impl LoomTextServiceRequestor {
    fn new(key: u64) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc().set_ivars(key)), init] }
    }
}

fn eligible(app: &App) -> bool {
    app.core.connected
        && app.core.pending_session_name.is_none()
        && !app.modal_captures_keyboard()
        && !app.core.overview.active
        && app
            .core
            .workspaces
            .active()
            .active_pane_id()
            .and_then(|pane| app.core.pane_grids.get(&pane))
            .is_some_and(|grid| !grid.password_input)
}

/// Preflight the selection before allocating, and never export concealed cells.
/// Use the shared copy path after checking so Services and Cmd+C agree on text.
fn selected_text(grid: &ClientPaneGrid, range: SelectionRange) -> Option<String> {
    if grid.password_input {
        return None;
    }
    let (a, b) = range;
    let (a, b) = if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    };
    if b.1 >= grid.total_lines() || grid.cols == 0 {
        return None;
    }
    let mut bytes = b.1.checked_sub(a.1)?;
    if bytes > MAX_TEXT_BYTES {
        return None;
    }
    for row in a.1..=b.1 {
        let left = if row == a.1 { a.0 } else { 0 };
        let right = if row == b.1 { b.0 } else { grid.cols - 1 };
        let (left, right) = grid.snap_selection_to_wide_chars(row, left, right);
        for (col, cell) in grid
            .buffer_row(row)
            .iter()
            .enumerate()
            .take(right as usize + 1)
            .skip(left as usize)
        {
            if cell.flags_u16() & FLAG_HIDDEN != 0 {
                return None;
            }
            if cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                continue;
            }
            let grapheme = row
                .checked_mul(grid.cols as usize)
                .and_then(|index| index.checked_add(col))
                .and_then(|index| u32::try_from(index).ok())
                .and_then(|index| grid.grapheme_map.get(&index));
            bytes =
                bytes.checked_add(grapheme.map_or_else(|| cell.ch().len_utf8(), String::len))?;
            if bytes > MAX_TEXT_BYTES {
                return None;
            }
        }
    }
    let text = grid.text_in_range(a, b);
    (!text.is_empty() && text.len() <= MAX_TEXT_BYTES).then_some(text)
}

pub fn update(app: &App, view: &NSView, visible: bool) {
    let key = view as *const NSView as usize;
    if !visible || !eligible(app) {
        remove(view);
        return;
    }
    let Some(window) = &app.window else {
        return;
    };
    let Some(sender) = &app.core.server_tx else {
        remove(view);
        return;
    };
    let Some(pane) = app.core.workspaces.active().active_pane_id() else {
        return;
    };
    let Some(grid) = app.core.pane_grids.get(&pane) else {
        return;
    };
    let target = Target {
        window: window.id(),
        pane,
        session: app.core.session_name.clone(),
        sender: sender.clone(),
    };
    let stamp = app
        .core
        .selection
        .as_ref()
        .filter(|selection| selection.pane_id == pane)
        .map(|selection| (grid.content_revision(), (selection.start, selection.end)));
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let entry = state.entry(key, target);
        if entry.stamp != stamp {
            entry.selected = stamp.and_then(|(_, range)| selected_text(grid, range));
            entry.stamp = stamp;
        }
    });
}
pub fn remove(view: &NSView) {
    STATE.with(|state| {
        state.borrow_mut().remove(view as *const NSView as usize);
    });
}

fn plain_text(kind: &NSString) -> bool {
    // AppKit aliases the legacy name, but older Services may still request it.
    kind.to_string() == "NSStringPboardType" || kind == unsafe { NSPasteboardTypeString }
}
fn accepts(send: Option<&NSString>, receive: Option<&NSString>, selected: bool) -> bool {
    (send.is_some() || receive.is_some())
        && send.is_none_or(|kind| plain_text(kind) && selected)
        && receive.is_none_or(plain_text)
}
extern "C" fn valid_requestor(
    this: &AnyObject,
    _sel: Sel,
    send: Option<&NSString>,
    receive: Option<&NSString>,
) -> *mut AnyObject {
    let requestor = STATE.with(|state| {
        let state = state.borrow();
        let key = state.views.get(&(this as *const AnyObject as usize))?;
        let entry = state.entries.get(key)?;
        accepts(send, receive, entry.selected.is_some()).then(|| entry.requestor.clone())
    });
    if let Some(requestor) = requestor {
        return Retained::autorelease_ptr(requestor.into());
    }
    // Preserve AppKit's responder chain for types the terminal cannot handle.
    let result: Option<Retained<AnyObject>> = unsafe {
        msg_send![super(this, this.class().superclass().expect("terminal NSView superclass")), validRequestorForSendType: send, returnType: receive]
    };
    result
        .map(Retained::autorelease_ptr)
        .unwrap_or(std::ptr::null_mut())
}
fn write_selection(key: u64, pasteboard: &NSPasteboard, types: &NSArray<NSString>) -> bool {
    let Some(kind) = types.iter().find(|kind| plain_text(kind)) else {
        return false;
    };
    let text = STATE.with(|state| state.borrow().entries.get(&key)?.selected.clone());
    let Some(text) = text else {
        return false;
    };
    // This is the service's private pasteboard, never the user's clipboard.
    unsafe {
        pasteboard.declareTypes_owner(&NSArray::from_slice(&[&*kind]), None);
    }
    pasteboard.setString_forType(&NSString::from_str(&text), &kind)
}
fn read_selection(key: u64, pasteboard: &NSPasteboard) -> bool {
    let target = STATE.with(|state| {
        state
            .borrow()
            .entries
            .get(&key)
            .map(|entry| entry.target.clone())
    });
    let Some(target) = target else {
        return false;
    };
    let text = pasteboard
        .stringForType(unsafe { NSPasteboardTypeString })
        .or_else(|| pasteboard.stringForType(&NSString::from_str("NSStringPboardType")));
    let Some(text) = text.filter(|text| text.len_utf16() <= MAX_TEXT_BYTES) else {
        return false;
    };
    let text = text.to_string();
    if text.is_empty() || text.len() > MAX_TEXT_BYTES {
        return false;
    }
    super::native::dispatch(super::Command::ServiceText(Request { target, text }));
    true
}
pub fn register(app: &NSApplication) {
    let types = NSArray::from_slice(&[
        unsafe { NSPasteboardTypeString },
        &NSString::from_str("NSStringPboardType"),
    ]);
    app.registerServicesMenuSendTypes_returnTypes(&types, &types);
}
pub fn install(class: &mut ClassBuilder) {
    // SAFETY: NSServicesMenuRequestor/NSResponder selectors with documented
    // Objective-C signatures. The view subclass has no additional ivars.
    unsafe {
        class.add_method(
            sel!(validRequestorForSendType:returnType:),
            valid_requestor as extern "C" fn(_, _, _, _) -> _,
        );
    }
}
impl super::MacApplication {
    pub(super) fn handle_service_text(&mut self, request: Request) {
        let Some(app) = self
            .windows
            .iter_mut()
            .find(|app| request.target.matches(app))
        else {
            return;
        };
        // Preserve bracketed paste and the existing large-paste confirmation.
        // A terminal selection is output, so returned text is terminal input,
        // never an attempt to rewrite the displayed output buffer.
        app.handle_text_paste(request.text);
        app.schedule_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_type_validation_handles_send_receive_and_selection() {
        let text = NSString::from_str("public.utf8-plain-text");
        let image = NSString::from_str("public.png");
        assert!(accepts(Some(&text), None, true));
        assert!(!accepts(Some(&text), None, false));
        assert!(accepts(None, Some(&text), false));
        assert!(accepts(Some(&text), Some(&text), true));
        assert!(!accepts(Some(&text), Some(&image), true));
        assert!(!accepts(Some(&image), None, true));
        assert!(!accepts(None, None, true));
    }
    #[test]
    fn service_selection_matches_copy_and_excludes_secrets_and_oversized_text() {
        let mut grid = ClientPaneGrid::new(3, 1, 0);
        grid.viewport[0].set_ch('中');
        grid.viewport[1].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        grid.viewport[2].set_ch('e');
        grid.grapheme_map.insert(2, "e\u{301}".into());
        assert_eq!(
            selected_text(&grid, ((2, 0), (1, 0))),
            Some("中e\u{301}".into())
        );
        grid.viewport[2].flags = FLAG_HIDDEN.to_le_bytes();
        assert!(selected_text(&grid, ((0, 0), (2, 0))).is_none());
        grid.viewport[2].flags = 0u16.to_le_bytes();
        grid.password_input = true;
        assert!(selected_text(&grid, ((0, 0), (0, 0))).is_none());
        grid.password_input = false;
        grid.grapheme_map.insert(2, "a".repeat(MAX_TEXT_BYTES + 1));
        assert!(selected_text(&grid, ((2, 0), (2, 0))).is_none());
    }
    #[test]
    fn target_identity_changes_on_session_or_connection_replacement() {
        let (sender, _) = crossbeam_channel::unbounded();
        let target = Target {
            window: WindowId::from(1),
            pane: 42,
            session: "one".into(),
            sender,
        };
        assert!(target.same(&target.clone()));
        let mut changed = target.clone();
        changed.session = "two".into();
        assert!(!target.same(&changed));
        changed.session = target.session.clone();
        changed.sender = crossbeam_channel::unbounded().0;
        assert!(!target.same(&changed));
        changed = target.clone();
        changed.pane += 1;
        assert!(!target.same(&changed));
    }
    #[test]
    fn retained_requestor_never_follows_view_to_another_pane_or_back() {
        let mut state = State::default();
        let target = Target {
            window: WindowId::from(1),
            pane: 42,
            session: "one".into(),
            sender: crossbeam_channel::unbounded().0,
        };
        let old = state.entry(123, target.clone()).requestor.clone();
        assert_eq!(
            *old.ivars(),
            *state.entry(123, target.clone()).requestor.ivars()
        );
        let mut other = target.clone();
        other.pane = 43;
        let new = state.entry(123, other).requestor.clone();
        assert_ne!(*old.ivars(), *new.ivars());
        assert!(!state.entries.contains_key(old.ivars()));
        let restored = state.entry(123, target).requestor.clone();
        assert_ne!(*old.ivars(), *restored.ivars());
        assert!(!state.entries.contains_key(new.ivars()));
        state.remove(123);
        assert!(state.entries.is_empty());
    }
}
