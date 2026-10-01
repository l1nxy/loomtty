//! AppKit accessibility objects for the GPU chrome. Native callbacks enqueue
//! immutable requests; the event loop revalidates them against the current UI.
use crate::app::{
    App,
    ui::accessibility::{Model, Node, Operation, Role, Value},
};
use crossbeam_channel::Sender;
use loom_protocol::message::ClientMessage;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_app_kit::{NSAccessibilityElement, NSAccessibilityPostNotification, NSView};
use objc2_foundation::{NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use winit::window::WindowId;

#[derive(Clone, Debug)]
pub(crate) struct Request {
    key: u64,
    revision: u64,
    operation: Operation,
}
struct Snapshot {
    key: u64,
    revision: u64,
    window: WindowId,
    session: String,
    sender: Option<Sender<ClientMessage>>,
    parent: Retained<NSView>,
    model_identity: u64,
    node: Node,
    can_cancel: bool,
    frame: NSRect,
    focused: bool,
}
struct Entry {
    snapshot: Rc<Snapshot>,
    element: Retained<LoomAccessibleControl>,
}
#[derive(Default)]
struct Root {
    label: String,
    keys: Vec<u64>,
    selected: Option<u64>,
    model: Option<Model>,
    hash: Option<u64>,
    announce_focus: bool,
    active: bool,
    frame: NSRect,
    scale: f64,
    session: String,
    sender: Option<Sender<ClientMessage>>,
}
#[derive(Default)]
struct State {
    next_key: u64,
    roots: HashMap<usize, Root>,
    entries: HashMap<u64, Entry>,
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }
fn snapshot(key: u64) -> Option<Rc<Snapshot>> {
    let snapshot = STATE.with(|state| state.borrow().entries.get(&key).map(|e| e.snapshot.clone()));
    if let Some(snapshot) = &snapshot {
        super::element::touch(Retained::as_ptr(&snapshot.parent) as usize);
    }
    snapshot
}
fn request(key: u64, operation: Operation) -> bool {
    let Some(s) = snapshot(key) else {
        return false;
    };
    if !s.node.enabled {
        return false;
    }
    if matches!(operation, Operation::Cancel) && !s.can_cancel {
        return false;
    }
    if matches!(operation, Operation::Focus) && s.node.edit.is_none() {
        return false;
    }
    super::super::native::dispatch(super::super::Command::Accessibility(super::Action::Chrome(
        Request {
            key,
            revision: s.revision,
            operation,
        },
    )));
    true
}
fn value(key: u64) -> Option<Retained<AnyObject>> {
    match &snapshot(key)?.node.value {
        Value::None => None,
        Value::Text(text) => Some(NSString::from_str(text).into()),
        Value::Bool(value) => Some(NSNumber::new_bool(*value).into()),
        Value::Number { value, .. } => Some(NSNumber::new_usize(*value).into()),
    }
}
fn role(key: u64) -> &'static str {
    match snapshot(key).map(|s| s.node.role) {
        Some(Role::Button) => "AXButton",
        Some(Role::Checkbox) => "AXCheckBox",
        Some(Role::Popup) => "AXPopUpButton",
        Some(Role::TextField) => "AXTextField",
        Some(Role::Scrollbar) => "AXScrollBar",
        _ => "AXStaticText",
    }
}
define_class!(
    #[unsafe(super = NSAccessibilityElement)]
    #[ivars = u64]
    struct LoomAccessibleControl;
    unsafe impl NSObjectProtocol for LoomAccessibleControl {}
    impl LoomAccessibleControl {
        #[unsafe(method(isAccessibilityElement))]
        fn alive(&self) -> bool { snapshot(*self.ivars()).is_some() }
        #[unsafe(method_id(accessibilityRole))]
        fn role(&self) -> Retained<NSString> { NSString::from_str(role(*self.ivars())) }
        #[unsafe(method_id(accessibilityLabel))]
        fn label(&self) -> Retained<NSString> { NSString::from_str(&snapshot(*self.ivars()).map(|s| s.node.label.clone()).unwrap_or_default()) }
        #[unsafe(method_id(accessibilityHelp))]
        fn help(&self) -> Retained<NSString> { NSString::from_str(&snapshot(*self.ivars()).map(|s| s.node.help.clone()).unwrap_or_default()) }
        #[unsafe(method_id(accessibilityIdentifier))]
        fn identifier(&self) -> Retained<NSString> { NSString::from_str(&format!("loom-control-{}", self.ivars())) }
        #[unsafe(method_id(accessibilityParent))]
        fn parent(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).map(|s| s.parent.clone().into()) }
        #[unsafe(method_id(accessibilityWindow))]
        fn window(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).and_then(|s| s.parent.window()).map(Into::into) }
        #[unsafe(method_id(accessibilityTopLevelUIElement))]
        fn top_level(&self) -> Option<Retained<AnyObject>> { snapshot(*self.ivars()).and_then(|s| s.parent.window()).map(Into::into) }
        #[unsafe(method(accessibilityFrame))]
        fn frame(&self) -> NSRect { snapshot(*self.ivars()).map(|s| s.frame).unwrap_or_default() }
        #[unsafe(method(isAccessibilityEnabled))]
        fn enabled(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.enabled) }
        #[unsafe(method(isAccessibilityFocused))]
        fn focused(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.focused) }
        #[unsafe(method(setAccessibilityFocused:))]
        fn set_focused(&self, focused: bool) { if focused { request(*self.ivars(), Operation::Focus); } }
        #[unsafe(method(isAccessibilitySelected))]
        fn selected(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.selected) }
        #[unsafe(method_id(accessibilityValue))]
        fn value(&self) -> Option<Retained<AnyObject>> { value(*self.ivars()) }
        #[unsafe(method(setAccessibilityValue:))]
        fn set_value(&self, value: &AnyObject) {
            let Some(s) = snapshot(*self.ivars()) else { return; };
            if s.node.edit.is_some() {
                if let Some(text) = value.downcast_ref::<NSString>().filter(|s| s.len_utf16() <= 16 * 1024) {
                    let text = text.to_string();
                    if text.len() <= 16 * 1024 { request(*self.ivars(), Operation::Text(text)); }
                }
            } else if s.node.scroll.is_some() && let Some(number) = value.downcast_ref::<NSNumber>() {
                let value = number.doubleValue();
                if value.is_finite() && value >= 0.0 { request(*self.ivars(), Operation::Number(value as usize)); }
            }
        }
        #[unsafe(method_id(accessibilityMinValue))]
        fn min_value(&self) -> Option<Retained<NSNumber>> { snapshot(*self.ivars()).filter(|s| s.node.scroll.is_some()).map(|_| NSNumber::new_usize(0)) }
        #[unsafe(method_id(accessibilityMaxValue))]
        fn max_value(&self) -> Option<Retained<NSNumber>> { snapshot(*self.ivars()).and_then(|s| match s.node.value { Value::Number { max, .. } => Some(NSNumber::new_usize(max)), _ => None }) }
        #[unsafe(method(accessibilityOrientation))]
        fn orientation(&self) -> isize {
            if snapshot(*self.ivars()).is_some_and(|s| s.node.role == Role::Scrollbar) {
                objc2_app_kit::NSAccessibilityOrientation::Vertical.0
            } else { objc2_app_kit::NSAccessibilityOrientation::Unknown.0 }
        }
        #[unsafe(method(accessibilityPerformPress))]
        fn press(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.press.is_some()) && request(*self.ivars(), Operation::Press) }
        #[unsafe(method(accessibilityPerformShowMenu))]
        fn show_menu(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.role == Role::Popup) && request(*self.ivars(), Operation::Press) }
        #[unsafe(method(accessibilityPerformIncrement))]
        fn increment(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.scroll.is_some()) && request(*self.ivars(), Operation::Step(1)) }
        #[unsafe(method(accessibilityPerformDecrement))]
        fn decrement(&self) -> bool { snapshot(*self.ivars()).is_some_and(|s| s.node.scroll.is_some()) && request(*self.ivars(), Operation::Step(-1)) }
        #[unsafe(method(accessibilityPerformCancel))]
        fn cancel(&self) -> bool { request(*self.ivars(), Operation::Cancel) }
        #[unsafe(method(isAccessibilitySelectorAllowed:))]
        fn allowed(&self, selector: Sel) -> bool { allowed(*self.ivars(), selector) }
    }
);
impl LoomAccessibleControl {
    fn new(key: u64) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc().set_ivars(key)), init] }
    }
}
fn allowed(key: u64, selector: Sel) -> bool {
    let Some(s) = snapshot(key) else {
        return false;
    };
    if selector == sel!(setAccessibilityFocused:) {
        return s.node.edit.is_some();
    }
    if selector == sel!(accessibilityPerformCancel) {
        return s.can_cancel;
    }
    if selector == sel!(setAccessibilityValue:) {
        return s.node.edit.is_some() || s.node.scroll.is_some();
    }
    if selector == sel!(accessibilityPerformPress) {
        return s.node.press.is_some();
    }
    if selector == sel!(accessibilityPerformShowMenu) {
        return s.node.role == Role::Popup;
    }
    if [
        sel!(accessibilityPerformIncrement),
        sel!(accessibilityPerformDecrement),
    ]
    .contains(&selector)
    {
        return s.node.scroll.is_some();
    }
    if [
        sel!(setAccessibilitySelected:),
        sel!(setAccessibilitySelectedText:),
        sel!(setAccessibilitySelectedTextRange:),
    ]
    .contains(&selector)
    {
        return false;
    }
    true
}
fn same_sender(a: &Option<Sender<ClientMessage>>, b: &Option<Sender<ClientMessage>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.same_channel(b),
        (None, None) => true,
        _ => false,
    }
}
fn notify(element: &AnyObject, name: &str) {
    unsafe {
        NSAccessibilityPostNotification(element, &NSString::from_str(name));
    }
}
pub fn update(app: &App, view: &NSView, visible: bool, focused: bool) {
    let root_key = view as *const NSView as usize;
    let Some(window) = &app.window else {
        return;
    };
    let Some(native) = view.window() else {
        return;
    };
    if !visible {
        remove(view);
        return;
    }
    let interested = super::element::interested(root_key);
    if !interested
        && STATE.with(|state| {
            state
                .borrow()
                .roots
                .get(&root_key)
                .is_none_or(|root| root.model.is_none())
        })
    {
        return;
    }
    let hash = Model::cache_key(app);
    let frame = native.convertRectToScreen(view.convertRect_toView(view.bounds(), None));
    if STATE.with(|state| {
        state.borrow().roots.get(&root_key).is_some_and(|root| {
            root.hash == Some(hash)
                && !root.announce_focus
                && root.active == focused
                && root.frame == frame
                && root.scale == app.dpi_scale
                && root.session == app.core.session_name
                && same_sender(&root.sender, &app.core.server_tx)
        })
    }) {
        return;
    }
    let cached = STATE.with(|state| {
        state
            .borrow()
            .roots
            .get(&root_key)
            .filter(|root| root.hash == Some(hash))
            .map(|root| root.model.clone())
    });
    let model = cached.unwrap_or_else(|| Model::capture(app));
    let mut notifications: Vec<(Retained<AnyObject>, &'static str)> = vec![];
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let mut root = state.roots.remove(&root_key).unwrap_or_default();
        let old_keys = root.keys.clone();
        let old_focused = old_keys.iter().copied().find(|key| {
            state
                .entries
                .get(key)
                .is_some_and(|entry| entry.snapshot.focused)
        });
        let old_label = root.label.clone();
        let scale = app.dpi_scale.max(0.1);
        let mut keys = Vec::new();
        if let Some(model) = &model {
            let mut default_key = None;
            for node in &model.nodes {
                let existing = old_keys.iter().copied().find(|key| {
                    state.entries.get(key).is_some_and(|e| {
                        e.snapshot.model_identity == model.identity
                            && e.snapshot.node.same_target(node)
                            && e.snapshot.session == app.core.session_name
                            && same_sender(&e.snapshot.sender, &app.core.server_tx)
                    })
                });
                let key = existing.unwrap_or_else(|| {
                    state.next_key += 1;
                    state.next_key
                });
                if node.default_focus {
                    default_key = Some(key);
                }
                let rect = NSRect::new(
                    NSPoint::new(node.bounds[0] as f64 / scale, node.bounds[1] as f64 / scale),
                    NSSize::new(node.bounds[2] as f64 / scale, node.bounds[3] as f64 / scale),
                );
                let clipped = super::element::intersection(rect, view.bounds());
                let frame = if clipped.size.width > 0.0 && clipped.size.height > 0.0 {
                    native.convertRectToScreen(view.convertRect_toView(clipped, None))
                } else {
                    NSRect::default()
                };
                let old = state.entries.remove(&key);
                let (element, revision) = old
                    .as_ref()
                    .map(|old| {
                        (
                            old.element.clone(),
                            old.snapshot.revision
                                + u64::from(
                                    old.snapshot.node.value != node.value
                                        || old.snapshot.node.enabled != node.enabled,
                                ),
                        )
                    })
                    .unwrap_or_else(|| (LoomAccessibleControl::new(key), 1));
                if interested
                    && old
                        .as_ref()
                        .is_some_and(|old| old.snapshot.node.value != node.value)
                {
                    notifications.push((element.clone().into(), "AXValueChanged"));
                }
                if interested
                    && old
                        .as_ref()
                        .is_some_and(|old| old.snapshot.node.selected != node.selected)
                {
                    notifications.push((element.clone().into(), "AXSelectedChildrenChanged"));
                }
                let snapshot = Rc::new(Snapshot {
                    key,
                    revision,
                    window: window.id(),
                    session: app.core.session_name.clone(),
                    sender: app.core.server_tx.clone(),
                    parent: Retained::from(view),
                    model_identity: model.identity,
                    node: node.clone(),
                    can_cancel: model.dismiss.is_some(),
                    frame,
                    focused: false,
                });
                state.entries.insert(key, Entry { snapshot, element });
                keys.push(key);
            }
            if !root.selected.is_some_and(|key| keys.contains(&key)) {
                root.selected = default_key;
            }
            if focused
                && let Some(key) = root.selected
                && let Some(entry) = state.entries.get_mut(&key)
            {
                Rc::get_mut(&mut entry.snapshot)
                    .expect("new snapshot")
                    .focused = true;
                if root.announce_focus || old_label != model.label || old_focused != Some(key) {
                    notifications.push((entry.element.clone().into(), "AXFocusedUIElementChanged"));
                }
                root.announce_focus = false;
            }
            root.label = model.label.clone();
            if focused && root.selected.is_none() && old_label != model.label {
                notifications.push((
                    Retained::<NSView>::from(view).into(),
                    "AXFocusedUIElementChanged",
                ));
            }
        } else {
            root.label.clear();
            root.selected = None;
        }
        for key in old_keys.iter().filter(|key| !keys.contains(key)) {
            if let Some(entry) = state.entries.remove(key) {
                notifications.push((entry.element.into(), "AXUIElementDestroyed"));
            }
        }
        if old_keys != keys {
            notifications.push((Retained::<NSView>::from(view).into(), "AXLayoutChanged"));
        }
        root.keys = keys;
        root.model = model;
        root.hash = Some(hash);
        root.active = focused;
        root.frame = frame;
        root.scale = app.dpi_scale;
        root.session = app.core.session_name.clone();
        root.sender = app.core.server_tx.clone();
        state.roots.insert(root_key, root);
    });
    for (element, name) in notifications {
        notify(&element, name);
    }
}
pub fn remove(view: &NSView) {
    let root = view as *const NSView as usize;
    let elements = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state
            .roots
            .remove(&root)
            .map(|root| {
                root.keys
                    .into_iter()
                    .filter_map(|key| state.entries.remove(&key).map(|entry| entry.element))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    for element in elements {
        notify(&element, "AXUIElementDestroyed");
    }
}
pub fn label(root: usize) -> Option<String> {
    STATE.with(|state| {
        state
            .borrow()
            .roots
            .get(&root)
            .filter(|r| !r.label.is_empty())
            .map(|r| r.label.clone())
    })
}
pub fn children(root: usize) -> Vec<Retained<AnyObject>> {
    STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&root)
            .map(|r| {
                r.keys
                    .iter()
                    .filter_map(|key| state.entries.get(key).map(|e| e.element.clone().into()))
                    .collect()
            })
            .unwrap_or_default()
    })
}
pub fn scrollbar(root: usize) -> Option<Retained<AnyObject>> {
    STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&root)?
            .keys
            .iter()
            .filter_map(|key| state.entries.get(key))
            .find(|entry| entry.snapshot.node.role == Role::Scrollbar)
            .map(|entry| entry.element.clone().into())
    })
}
pub fn focused(root: usize) -> Option<Retained<AnyObject>> {
    STATE.with(|state| {
        let state = state.borrow();
        let key = state.roots.get(&root)?.selected?;
        state
            .entries
            .get(&key)
            .filter(|e| e.snapshot.focused)
            .map(|e| e.element.clone().into())
    })
}
pub fn has_focus(root: usize) -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .roots
            .get(&root)
            .is_some_and(|root| root.active && root.model.is_some())
    })
}
pub fn hit(root: usize, point: NSPoint) -> Option<Retained<AnyObject>> {
    STATE.with(|state| {
        let state = state.borrow();
        state
            .roots
            .get(&root)?
            .keys
            .iter()
            .rev()
            .filter_map(|key| state.entries.get(key))
            .find(|e| super::element::contains(e.snapshot.frame, point))
            .map(|e| e.element.clone().into())
    })
}
pub fn cancel(root: usize) -> bool {
    let key = STATE.with(|state| {
        state
            .borrow()
            .roots
            .get(&root)
            .and_then(|r| r.keys.first().copied())
    });
    key.is_some_and(|key| request(key, Operation::Cancel))
}
impl super::super::MacApplication {
    pub(super) fn handle_chrome_accessibility(&mut self, request: Request) {
        let Some(s) = snapshot(request.key).filter(|s| {
            !matches!(request.operation, Operation::Text(_)) || s.revision == request.revision
        }) else {
            return;
        };
        let Some(app) = self.windows.iter_mut().find(|app| {
            app.window.as_ref().is_some_and(|w| w.id() == s.window)
                && app.core.session_name == s.session
                && same_sender(&app.core.server_tx, &s.sender)
        }) else {
            return;
        };
        if !Model::apply(app, s.model_identity, &s.node, request.operation.clone()) {
            return;
        }
        if matches!(request.operation, Operation::Focus) {
            STATE.with(|state| {
                if let Some(root) = state
                    .borrow_mut()
                    .roots
                    .get_mut(&(Retained::as_ptr(&s.parent) as usize))
                {
                    root.selected = Some(s.key);
                    root.announce_focus = true;
                }
            });
            if let Some(native) = s.parent.window() {
                native.makeKeyAndOrderFront(None);
            }
            let mtm =
                objc2::MainThreadMarker::new().expect("accessibility runs on the main thread");
            #[allow(deprecated)]
            objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            self.active = Some(s.window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detached_control_callbacks_are_valid_and_cannot_mutate_ui() {
        let element = LoomAccessibleControl::new(u64::MAX);
        let role: Retained<NSString> = unsafe { msg_send![&element, accessibilityRole] };
        let value: Option<Retained<AnyObject>> = unsafe { msg_send![&element, accessibilityValue] };
        let alive: bool = unsafe { msg_send![&element, isAccessibilityElement] };
        let press: bool = unsafe { msg_send![&element, accessibilityPerformPress] };
        let editable: bool = unsafe {
            msg_send![&element, isAccessibilitySelectorAllowed: sel!(setAccessibilityValue:)]
        };
        assert_eq!(role.to_string(), "AXStaticText");
        assert!(value.is_none());
        assert!(!alive && !press && !editable);
    }
}
