//! AppKit callbacks only queue commands; they never re-enter the Rust app.
//!
//! The pinned winit 0.30 fork requires its original application delegate (and
//! checks isKindOfClass internally), despite the custom-delegate example in
//! its platform docs. Extend that object's class without adding ivars. This
//! preserves all winit launch, termination, event-loop state and ownership.
use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationTerminateReply, NSView, NSWindow, NSWindowOrderingMode,
};
use objc2_foundation::{NSString, NSURL};
use std::cell::RefCell;
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::Command;

struct DelegateState {
    tx: Sender<Command>,
    proxy: EventLoopProxy<()>,
}

thread_local! {
    static CALLBACKS: RefCell<Option<DelegateState>> = const { RefCell::new(None) };
}

fn dispatch(command: Command) {
    CALLBACKS.with(|state| {
        if let Some(state) = &*state.borrow() {
            let _ = state.tx.send(command);
            let _ = state.proxy.send_event(());
        }
    });
}

extern "C" fn reopen(_this: &AnyObject, _sel: Sel, _app: &NSApplication, _visible: Bool) -> Bool {
    dispatch(Command::Reopen);
    Bool::YES
}

extern "C" fn terminate_after_last_window(
    _this: &AnyObject,
    _sel: Sel,
    _app: &NSApplication,
) -> Bool {
    Bool::NO
}

extern "C" fn terminate(
    _this: &AnyObject,
    _sel: Sel,
    _app: &NSApplication,
) -> NSApplicationTerminateReply {
    dispatch(Command::SystemQuit);
    NSApplicationTerminateReply::TerminateLater
}

extern "C" fn new_window_for_tab(_this: &AnyObject, _sel: Sel, _sender: Option<&AnyObject>) {
    dispatch(Command::NewTab);
}

pub struct Delegate {
    object: Retained<AnyObject>,
    original_class: &'static AnyClass,
}

impl Delegate {
    pub fn install(tx: Sender<Command>, proxy: EventLoopProxy<()>) -> Self {
        let mtm = MainThreadMarker::new().expect("macOS UI must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        let delegate = app.delegate().expect("winit application delegate");
        // SAFETY: Any protocol object is also an Objective-C object; retain
        // ownership and do not change its layout or winit's ivars.
        let object: Retained<AnyObject> = unsafe { Retained::cast_unchecked(delegate) };
        let original_class = object.class();
        let mut class = ClassBuilder::new(c"LoomApplicationDelegate", original_class)
            .expect("register loom application delegate extension once");
        // SAFETY: selectors use AppKit's documented ABIs. The subclass adds
        // no ivars and inherits winit's implementation of every other method.
        unsafe {
            class.add_method(
                sel!(applicationShouldHandleReopen:hasVisibleWindows:),
                reopen as extern "C" fn(_, _, _, _) -> _,
            );
            class.add_method(
                sel!(applicationShouldTerminateAfterLastWindowClosed:),
                terminate_after_last_window as extern "C" fn(_, _, _) -> _,
            );
            class.add_method(
                sel!(applicationShouldTerminate:),
                terminate as extern "C" fn(_, _, _) -> _,
            );
            class.add_method(
                sel!(newWindowForTab:),
                new_window_for_tab as extern "C" fn(_, _, _),
            );
        }
        let class = class.register();
        assert_eq!(original_class.instance_size(), class.instance_size());
        CALLBACKS.with(|state| *state.borrow_mut() = Some(DelegateState { tx, proxy }));
        // SAFETY: same instance layout, original superclass/ivars preserved,
        // and only the main thread accesses this main-thread-only delegate.
        unsafe {
            AnyObject::set_class(&object, class);
        }
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        Self {
            object,
            original_class,
        }
    }
}

impl Drop for Delegate {
    fn drop(&mut self) {
        // SAFETY: restore the class we extended before releasing our retain.
        unsafe {
            AnyObject::set_class(&self.object, self.original_class);
        }
        CALLBACKS.with(|state| *state.borrow_mut() = None);
    }
}

pub fn native_window(window: &Window) -> Option<Retained<NSWindow>> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    let _mtm = MainThreadMarker::new()?;
    // SAFETY: winit supplies a live NSView, and the Window borrow outlives
    // this access. Retaining its NSWindow keeps the return value valid.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

pub fn join_tab(parent: &Window, child: &Window) {
    if let (Some(parent), Some(child)) = (native_window(parent), native_window(child)) {
        parent.addTabbedWindow_ordered(&child, NSWindowOrderingMode::Above);
        child.makeKeyAndOrderFront(None);
    }
}

pub fn select_tab(window: &Window, next: bool) {
    if let Some(window) = native_window(window) {
        if next {
            window.selectNextTab(None);
        } else {
            window.selectPreviousTab(None);
        }
    }
}

// Carbon's Secure Event Input is process-wide and reference counted. Only
// balance successful calls owned by this guard; never disable another app's.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn EnableSecureEventInput() -> i32;
    fn DisableSecureEventInput() -> i32;
}

#[derive(Default)]
pub struct SecureInput {
    enabled: bool,
}

impl SecureInput {
    pub fn update(&mut self, wanted: bool) {
        if self.enabled == wanted {
            return;
        }
        // SAFETY: Carbon API has no pointer arguments; called on main thread.
        let status = unsafe {
            if wanted {
                EnableSecureEventInput()
            } else {
                DisableSecureEventInput()
            }
        };
        if status == 0 {
            self.enabled = wanted;
        } else {
            log::warn!("could not change macOS Secure Input to {wanted}: {status}");
        }
    }
}

impl Drop for SecureInput {
    fn drop(&mut self) {
        self.update(false);
    }
}

pub fn set_directory(window: &Window, path: Option<&str>) {
    if let Some(window) = native_window(window) {
        let url = path
            .filter(|path| std::path::Path::new(path).is_absolute())
            .map(|path| NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true));
        window.setRepresentedURL(url.as_deref());
    }
}

/// Read the application appearance, which is independent of a fixed theme on
/// an individual window. Called from UI/config reload, never from server code.
pub(crate) fn system_dark_appearance() -> Option<bool> {
    use objc2_app_kit::{NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
    use objc2_foundation::NSArray;
    let mtm = MainThreadMarker::new()?;
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    // SAFETY: AppKit appearance names are immutable, process-lifetime constants.
    let (light, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let best = appearance.bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[light, dark]));
    best.map(|name| &*name == dark)
}
