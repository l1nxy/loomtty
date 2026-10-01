use super::{Action, Snapshot};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ClassBuilder, Sel};
use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSAccessibilityElement, NSAccessibilityPostNotification, NSView};
use objc2_foundation::{
    NSArray, NSAttributedString, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

#[derive(Default)]
struct Root {
    keys: Vec<u64>,
    label: String,
    interest: Option<Instant>,
}
#[derive(Default)]
struct State {
    roots: HashMap<usize, Root>,
    panes: HashMap<u64, Rc<Snapshot>>,
    elements: HashMap<u64, Retained<LoomAccessibleTerminal>>,
    interest: HashMap<u64, Instant>,
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }
pub(super) fn touch(root: usize) {
    let wake = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let root = state.roots.entry(root).or_default();
        let was_active = root.interest.is_some_and(|at| at.elapsed().as_secs() < 10);
        root.interest = Some(Instant::now());
        !was_active
    });
    if wake {
        super::super::native::dispatch(super::super::Command::AccessibilityRefresh);
    }
}
pub fn interested(root: usize) -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .roots
            .get(&root)
            .and_then(|root| root.interest)
            .is_some_and(|at| at.elapsed().as_secs() < 10)
    })
}
pub fn wants_history(key: u64) -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .interest
            .get(&key)
            .is_some_and(|at| at.elapsed().as_secs() < 10)
    })
}
pub fn snapshot(key: u64) -> Option<Rc<Snapshot>> {
    STATE.with(|state| state.borrow().panes.get(&key).cloned())
}
fn queried(key: u64) -> Option<Rc<Snapshot>> {
    let snapshot = snapshot(key)?;
    let wake = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let recent = state
            .interest
            .insert(key, Instant::now())
            .is_some_and(|at| at.elapsed().as_secs() < 10);
        !recent
    });
    touch(Retained::as_ptr(&snapshot.parent) as usize);
    if wake {
        super::super::native::dispatch(super::super::Command::AccessibilityRefresh);
    }
    Some(snapshot)
}
fn nsrange(range: std::ops::Range<usize>) -> NSRange {
    NSRange::new(range.start, range.end.saturating_sub(range.start))
}
fn missing_range() -> NSRange {
    NSRange::new(isize::MAX as usize, 0)
}
fn string(units: &[u16]) -> Retained<NSString> {
    // NSString preserves UTF-16 indexing, including a caller's partial range
    // through a surrogate pair. Never convert offsets into UTF-8 byte indices.
    unsafe {
        NSString::initWithCharacters_length(
            NSString::alloc(),
            std::ptr::NonNull::new(units.as_ptr().cast_mut()).unwrap(),
            units.len(),
        )
    }
}
fn focus(key: u64) -> bool {
    if snapshot(key).is_none() {
        return false;
    }
    super::super::native::dispatch(super::super::Command::Accessibility(Action::Focus { key }));
    true
}

fn text_for_range(key: u64, range: NSRange) -> Option<Retained<NSString>> {
    let s = queried(key)?;
    let range = s.text.checked_range(range.location, range.length)?;
    Some(string(&s.text.utf16[range]))
}

define_class!(
    #[unsafe(super = NSAccessibilityElement)]
    #[ivars = u64]
    pub struct LoomAccessibleTerminal;
    unsafe impl NSObjectProtocol for LoomAccessibleTerminal {}
    impl LoomAccessibleTerminal {
        #[unsafe(method(isAccessibilityElement))]
        fn is_element(&self) -> bool { snapshot(*self.ivars()).is_some() }
        #[unsafe(method_id(accessibilityRole))]
        fn role(&self) -> Retained<NSString> { NSString::from_str("AXTextArea") }
        #[unsafe(method_id(accessibilityRoleDescription))]
        fn role_description(&self) -> Retained<NSString> { NSString::from_str("terminal") }
        #[unsafe(method_id(accessibilityLabel))]
        fn label(&self) -> Retained<NSString> { NSString::from_str(&queried(*self.ivars()).map(|s| s.label.clone()).unwrap_or_default()) }
        #[unsafe(method_id(accessibilityHelp))]
        fn help(&self) -> Retained<NSString> {
            NSString::from_str(if queried(*self.ivars()).is_some_and(|s| s.text.clipped) {
                "Terminal output. Recent scrollback and visible rows are available. Focus this terminal to type; use the terminal scroll keys to read older history."
            } else { "Terminal output. Focus this terminal to type." })
        }
        #[unsafe(method_id(accessibilityIdentifier))]
        fn identifier(&self) -> Retained<NSString> { NSString::from_str(&format!("loom-terminal-{}", self.ivars())) }
        #[unsafe(method_id(accessibilityParent))]
        fn parent(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).map(|s| s.parent.clone().into()) }
        #[unsafe(method_id(accessibilityWindow))]
        fn window(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).and_then(|s| s.parent.window()).map(Into::into) }
        #[unsafe(method_id(accessibilityTopLevelUIElement))]
        fn top_level(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).and_then(|s| s.parent.window()).map(Into::into) }
        #[unsafe(method(accessibilityFrame))]
        fn frame(&self) -> NSRect { queried(*self.ivars()).map(|s| s.clip).unwrap_or_default() }
        #[unsafe(method(isAccessibilityFocused))]
        fn focused(&self) -> bool { queried(*self.ivars()).is_some_and(|s| s.focused) }
        #[unsafe(method(setAccessibilityFocused:))]
        fn set_focused(&self, value: bool) { if value { focus(*self.ivars()); } }
        #[unsafe(method(isAccessibilityEnabled))]
        fn enabled(&self) -> bool { queried(*self.ivars()).is_some_and(|s| s.enabled) }
        #[unsafe(method(accessibilityPerformPress))]
        fn press(&self) -> bool { focus(*self.ivars()) }
        #[unsafe(method_id(accessibilityValue))]
        fn value(&self) -> Retained<NSString> { queried(*self.ivars()).map(|s| string(&s.text.utf16)).unwrap_or_else(|| NSString::from_str("")) }
        #[unsafe(method(accessibilityNumberOfCharacters))]
        fn length(&self) -> isize { queried(*self.ivars()).map(|s| s.text.utf16.len() as isize).unwrap_or(0) }
        #[unsafe(method(accessibilityVisibleCharacterRange))]
        fn visible_range(&self) -> NSRange { queried(*self.ivars()).map(|s| nsrange(s.text.visible.clone())).unwrap_or_default() }
        #[unsafe(method(accessibilitySelectedTextRange))]
        fn selected_range(&self) -> NSRange { queried(*self.ivars()).map(|s| nsrange(s.text.selected.clone())).unwrap_or_default() }
        #[unsafe(method_id(accessibilitySelectedText))]
        fn selected_text(&self) -> Retained<NSString> {
            queried(*self.ivars()).map(|s| string(&s.text.utf16[s.text.selected.clone()])).unwrap_or_else(|| NSString::from_str(""))
        }
        #[unsafe(method(setAccessibilitySelectedTextRange:))]
        fn select(&self, range: NSRange) {
            let Some(s) = queried(*self.ivars()) else { return; };
            if s.text.checked_range(range.location, range.length).is_some() {
                super::super::native::dispatch(super::super::Command::Accessibility(Action::Select { key: *self.ivars(), revision: s.revision, range }));
            }
        }
        #[unsafe(method(accessibilityInsertionPointLineNumber))]
        fn insertion_line(&self) -> isize { queried(*self.ivars()).and_then(|s| s.text.cursor_line).map(|line| line as isize).unwrap_or(-1) }
        #[unsafe(method_id(accessibilityStringForRange:))]
        fn text_for_range(&self, range: NSRange) -> Option<Retained<NSString>> {
            text_for_range(*self.ivars(), range)
        }
        #[unsafe(method_id(accessibilityAttributedStringForRange:))]
        fn attributed(&self, range: NSRange) -> Option<Retained<NSAttributedString>> {
            text_for_range(*self.ivars(), range).map(|s| NSAttributedString::initWithString(NSAttributedString::alloc(), &s))
        }
        #[unsafe(method(accessibilityLineForIndex:))]
        fn line_for_index(&self, index: isize) -> isize { queried(*self.ivars()).and_then(|s| s.text.line_for_index(index as usize)).map(|n| n as isize).unwrap_or(-1) }
        #[unsafe(method(accessibilityRangeForLine:))]
        fn range_for_line(&self, line: isize) -> NSRange {
            queried(*self.ivars()).and_then(|s| s.text.lines.get(line as usize).map(|line| nsrange(line.range.clone()))).unwrap_or_else(missing_range)
        }
        #[unsafe(method(accessibilityRangeForIndex:))]
        fn range_for_index(&self, index: isize) -> NSRange {
            queried(*self.ivars()).and_then(|s| s.text.cell_for_index(index as usize).map(|(_, cell)| nsrange(cell.range.clone()))).unwrap_or_else(missing_range)
        }
        #[unsafe(method(accessibilityRangeForPosition:))]
        fn range_for_position(&self, point: NSPoint) -> NSRange {
            let Some(s) = queried(*self.ivars()) else { return missing_range(); };
            if !contains(s.clip, point) { return missing_range(); }
            let col = ((point.x - s.frame.origin.x) / s.cell.width).floor().max(0.0) as u16;
            let row = ((s.frame.origin.y + s.frame.size.height - point.y) / s.cell.height).floor().max(0.0) as usize + s.text.viewport_top;
            s.text.offset(col, row, false).zip(s.text.offset(col, row, true)).map(|(a,b)| nsrange(a..b)).unwrap_or_else(missing_range)
        }
        #[unsafe(method(accessibilityFrameForRange:))]
        fn frame_for_range(&self, range: NSRange) -> NSRect {
            queried(*self.ivars()).map(|s| frame_for_range(&s.text, s.frame, s.clip, s.cell, range)).unwrap_or_default()
        }
        #[unsafe(method(isAccessibilitySelectorAllowed:))]
        fn selector_allowed(&self, selector: Sel) -> bool {
            // The terminal buffer is not an editable text-document value.
            ![sel!(setAccessibilityValue:), sel!(setAccessibilitySelectedText:), sel!(setAccessibilitySelectedTextRanges:), sel!(setAccessibilityVisibleCharacterRange:)].contains(&selector)
        }
    }
);
impl LoomAccessibleTerminal {
    fn new(key: u64) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc().set_ivars(key)), init] }
    }
}
pub(super) fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}
pub(super) fn intersection(a: NSRect, b: NSRect) -> NSRect {
    let x = a.origin.x.max(b.origin.x);
    let y = a.origin.y.max(b.origin.y);
    let right = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let top = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    if right <= x || top <= y {
        return NSRect::default();
    }
    NSRect::new(NSPoint::new(x, y), NSSize::new(right - x, top - y))
}

fn frame_for_range(
    text: &super::text::Text,
    frame: NSRect,
    clip: NSRect,
    cell_size: NSSize,
    range: NSRange,
) -> NSRect {
    let Some(range) = text.checked_range(range.location, range.length) else {
        return NSRect::default();
    };
    if range.is_empty() {
        let Some((line, cell)) = text.cell_for_index(range.start) else {
            return NSRect::default();
        };
        if line.row < text.viewport_top {
            return NSRect::default();
        }
        let col = if range.start >= cell.range.end {
            cell.col + cell.width
        } else {
            cell.col
        };
        return intersection(
            NSRect::new(
                NSPoint::new(
                    frame.origin.x + col as f64 * cell_size.width,
                    frame.origin.y + frame.size.height
                        - (line.row - text.viewport_top + 1) as f64 * cell_size.height,
                ),
                NSSize::new(1.0, cell_size.height),
            ),
            clip,
        );
    }
    let mut bounds: Option<NSRect> = None;
    for line in &text.lines {
        if line.row < text.viewport_top {
            continue;
        }
        for cell in &line.cells {
            let hit = cell.range.start < range.end && cell.range.end > range.start;
            if !hit {
                continue;
            }
            let rect = NSRect::new(
                NSPoint::new(
                    frame.origin.x + cell.col as f64 * cell_size.width,
                    frame.origin.y + frame.size.height
                        - (line.row - text.viewport_top + 1) as f64 * cell_size.height,
                ),
                NSSize::new(cell.width as f64 * cell_size.width, cell_size.height),
            );
            bounds = Some(if let Some(old) = bounds {
                let x = old.origin.x.min(rect.origin.x);
                let y = old.origin.y.min(rect.origin.y);
                NSRect::new(
                    NSPoint::new(x, y),
                    NSSize::new(
                        (old.origin.x + old.size.width).max(rect.origin.x + rect.size.width) - x,
                        (old.origin.y + old.size.height).max(rect.origin.y + rect.size.height) - y,
                    ),
                )
            } else {
                rect
            });
        }
    }
    bounds
        .map(|rect| intersection(rect, clip))
        .unwrap_or_default()
}

extern "C" fn is_element(_this: &AnyObject, _sel: Sel) -> Bool {
    Bool::YES
}
extern "C" fn root_role(_this: &AnyObject, _sel: Sel) -> *mut NSString {
    Retained::autorelease_ptr(NSString::from_str("AXGroup"))
}
extern "C" fn root_label(this: &AnyObject, _sel: Sel) -> *mut NSString {
    let key = this as *const AnyObject as usize;
    touch(key);
    let label = super::chrome::label(key).unwrap_or_else(|| {
        STATE.with(|state| {
            state
                .borrow()
                .roots
                .get(&key)
                .map(|r| r.label.clone())
                .unwrap_or_else(|| "Terminal panes".into())
        })
    });
    Retained::autorelease_ptr(NSString::from_str(&label))
}
extern "C" fn children(this: &AnyObject, _sel: Sel) -> *mut NSArray<AnyObject> {
    let key = this as *const AnyObject as usize;
    touch(key);
    let mut children = STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&key)
            .map(|r| {
                r.keys
                    .iter()
                    .filter_map(|key| state.elements.get(key).cloned().map(Into::into))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    children.extend(super::chrome::children(key));
    Retained::autorelease_ptr(NSArray::from_retained_slice(&children))
}
extern "C" fn focused_child(this: &AnyObject, _sel: Sel) -> *mut AnyObject {
    let root = this as *const AnyObject as usize;
    touch(root);
    if let Some(element) = super::chrome::focused(root) {
        return Retained::autorelease_ptr(element);
    }
    let focused = STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&root)?
            .keys
            .iter()
            .find(|key| state.panes.get(key).is_some_and(|s| s.focused))
            .and_then(|key| state.elements.get(key))
            .cloned()
    });
    focused
        .map(|e| Retained::autorelease_ptr(e.into()))
        .unwrap_or_else(|| {
            if super::chrome::has_focus(root) {
                Retained::autorelease_ptr(this.retain())
            } else {
                std::ptr::null_mut()
            }
        })
}
extern "C" fn hit_test(this: &AnyObject, _sel: Sel, point: NSPoint) -> *mut AnyObject {
    let root = this as *const AnyObject as usize;
    touch(root);
    if let Some(element) = super::chrome::hit(root, point) {
        return Retained::autorelease_ptr(element);
    }
    let hit = STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&root)?
            .keys
            .iter()
            .find(|key| {
                state
                    .panes
                    .get(key)
                    .is_some_and(|s| contains(s.clip, point))
            })
            .and_then(|key| state.elements.get(key))
            .cloned()
    });
    hit.map(|e| Retained::autorelease_ptr(e.into()))
        .unwrap_or_else(|| Retained::autorelease_ptr(this.retain()))
}
pub fn install(class: &mut ClassBuilder) {
    // SAFETY: documented NSAccessibility/NSView selector ABIs, no added ivars.
    unsafe {
        class.add_method(
            sel!(accessibilityVerticalScrollBar),
            scrollbar as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityPerformCancel),
            cancel as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(isAccessibilityElement),
            is_element as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityRole),
            root_role as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityLabel),
            root_label as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityChildren),
            children as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityContents),
            children as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityFocusedUIElement),
            focused_child as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(accessibilityHitTest:),
            hit_test as extern "C" fn(_, _, _) -> _,
        );
    }
}
extern "C" fn scrollbar(this: &AnyObject, _sel: Sel) -> *mut AnyObject {
    super::chrome::scrollbar(this as *const AnyObject as usize)
        .map(Retained::autorelease_ptr)
        .unwrap_or(std::ptr::null_mut())
}
extern "C" fn cancel(this: &AnyObject, _sel: Sel) -> Bool {
    Bool::new(super::chrome::cancel(this as *const AnyObject as usize))
}
fn notify(element: &AnyObject, name: &str) {
    unsafe {
        NSAccessibilityPostNotification(element, &NSString::from_str(name));
    }
}
pub fn publish(view: &NSView, label: String, panes: Vec<Rc<Snapshot>>) {
    let root = view as *const NSView as usize;
    let interested = interested(root);
    let mut notifications: Vec<(Retained<AnyObject>, &'static str)> = Vec::new();
    let changed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let keys: Vec<_> = panes.iter().map(|s| s.key).collect();
        let old_keys = state.roots.entry(root).or_default().keys.clone();
        let changed = keys != old_keys;
        for key in old_keys.iter().filter(|key| !keys.contains(key)) {
            state.panes.remove(key);
            state.interest.remove(key);
            if let Some(element) = state.elements.remove(key) {
                notifications.push((element.into(), "AXUIElementDestroyed"));
            }
        }
        for pane in panes {
            let element = state
                .elements
                .entry(pane.key)
                .or_insert_with(|| LoomAccessibleTerminal::new(pane.key))
                .clone();
            if let Some(old) = state.panes.get(&pane.key) {
                if interested && old.revision != pane.revision && old.text.utf16 != pane.text.utf16
                {
                    notifications.push((element.clone().into(), "AXValueChanged"));
                }
                if interested
                    && (old.text.selected != pane.text.selected
                        || old.text.cursor_line != pane.text.cursor_line)
                {
                    notifications.push((element.clone().into(), "AXSelectedTextChanged"));
                }
                if !old.focused && pane.focused {
                    notifications.push((element.clone().into(), "AXFocusedUIElementChanged"));
                }
            } else if pane.focused {
                notifications.push((element.clone().into(), "AXFocusedUIElementChanged"));
            }
            state.panes.insert(pane.key, pane);
        }
        let data = state.roots.entry(root).or_default();
        data.keys = keys;
        data.label = label;
        changed
    });
    for (element, name) in notifications {
        notify(&element, name);
    }
    if changed {
        notify(view, "AXLayoutChanged");
    }
}
pub fn remove(view: &NSView) {
    let root = view as *const NSView as usize;
    publish(view, String::new(), Vec::new());
    STATE.with(|state| {
        state.borrow_mut().roots.remove(&root);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detached_elements_have_valid_objc_getters_and_no_editable_buffer() {
        let element = LoomAccessibleTerminal::new(u64::MAX);
        let role: Retained<NSString> = unsafe { msg_send![&element, accessibilityRole] };
        let value: Retained<NSString> = unsafe { msg_send![&element, accessibilityValue] };
        let range: NSRange = unsafe { msg_send![&element, accessibilitySelectedTextRange] };
        let allowed: bool = unsafe {
            msg_send![&element, isAccessibilitySelectorAllowed: sel!(setAccessibilityValue:)]
        };
        let alive: bool = unsafe { msg_send![&element, isAccessibilityElement] };
        assert_eq!(role.to_string(), "AXTextArea");
        assert!(value.is_empty());
        assert_eq!(range, NSRange::new(0, 0));
        assert!(!allowed);
        assert!(!alive);
    }
    #[test]
    fn range_geometry_uses_cocoa_y_direction_and_whole_unicode_cells() {
        let mut grid = crate::grid::ClientPaneGrid::new(4, 2, 0);
        grid.viewport[0].set_ch('中');
        grid.viewport[0].flags = loom_protocol::message::FLAG_WIDE_CHAR.to_le_bytes();
        grid.viewport[1].flags = loom_protocol::message::FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        grid.viewport[2].set_ch('😀');
        let text = super::super::text::Text::capture(&grid, None, false);
        let frame = NSRect::new(NSPoint::new(100.0, 200.0), NSSize::new(40.0, 40.0));
        let cell = NSSize::new(10.0, 20.0);
        assert_eq!(
            frame_for_range(&text, frame, frame, cell, NSRange::new(0, 1)),
            NSRect::new(NSPoint::new(100.0, 220.0), NSSize::new(20.0, 20.0))
        );
        assert_eq!(
            frame_for_range(&text, frame, frame, cell, NSRange::new(2, 1)),
            NSRect::new(NSPoint::new(120.0, 220.0), NSSize::new(10.0, 20.0))
        );
        assert_eq!(
            frame_for_range(&text, frame, frame, cell, NSRange::new(1, 0)),
            NSRect::new(NSPoint::new(120.0, 220.0), NSSize::new(1.0, 20.0))
        );
        assert_eq!(
            frame_for_range(&text, frame, frame, cell, NSRange::new(usize::MAX, 1)),
            NSRect::default()
        );
    }
    #[test]
    fn utf16_substring_preserves_appkit_length_even_inside_surrogate_pair() {
        let full = string(&[0xD83D, 0xDE00, 0x0065, 0x0301]);
        assert_eq!(full.len_utf16(), 4);
        assert_eq!(string(&[0xD83D]).len_utf16(), 1);
    }
}
