//! Preserve the user frame before AppKit's animated zoom starts emitting resize
//! events. `isZoomed` becomes true only at the end of that animation.
use std::cell::RefCell;
use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{Message, sel};
use objc2_app_kit::NSWindow;

use super::Frame;

#[derive(Clone)]
struct SavedFrame {
    frame: Frame,
    transitioning: bool,
}

thread_local! {
    static ORIGINAL: RefCell<Option<(&'static AnyClass, Imp)>> = const { RefCell::new(None) };
    static FRAMES: RefCell<HashMap<usize, SavedFrame>> = RefCell::default();
}

fn key(window: &NSWindow) -> usize {
    window as *const NSWindow as usize
}

fn members(window: &NSWindow) -> Vec<Retained<NSWindow>> {
    window
        .tabGroup()
        .map(|group| group.windows().to_vec())
        .filter(|windows| !windows.is_empty())
        .unwrap_or_else(|| vec![window.retain()])
}

extern "C-unwind" fn zoom(window: &NSWindow, _selector: Sel, sender: Option<&AnyObject>) {
    let windows = members(window);
    let frame = Frame::from(window.frame());
    let zoomed = window.isZoomed();
    let restore_frame = zoomed.then(|| normal_frame(window)).flatten();
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        for member in &windows {
            let saved = frames.entry(key(member)).or_insert_with(|| SavedFrame {
                frame: frame.clone(),
                transitioning: true,
            });
            if !zoomed {
                saved.frame = frame.clone();
            }
            saved.transitioning = true;
        }
    });
    // AppKit can run nested resize callbacks here. Do not retain a Rust borrow.
    let original = ORIGINAL.with(|original| original.borrow().as_ref().unwrap().1);
    // The saved IMP has precisely NSWindow's zoom: ABI.
    unsafe {
        let original: unsafe extern "C-unwind" fn(&NSWindow, Sel, Option<&AnyObject>) =
            std::mem::transmute(original);
        original(window, sel!(zoom:), sender);
    }
    if !window.isZoomed()
        && let Some(frame) = restore_frame
    {
        // AppKit's per-window user frame can differ between native tabs after
        // restoration. Restore the group's recorded frame when unzooming any tab.
        window.setFrame_display((&frame).into(), true);
    }
    let zoomed = window.isZoomed();
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        for member in &windows {
            if !zoomed {
                frames.remove(&key(member));
            } else if let Some(saved) = frames.get_mut(&key(member)) {
                saved.transitioning = false;
            }
        }
    });
}

pub(super) fn normal_frame(window: &NSWindow) -> Option<Frame> {
    let windows = members(window);
    let zoomed = windows.iter().any(|window| window.isZoomed());
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        let saved = windows.iter().find_map(|window| frames.get(&key(window)))?;
        if zoomed || saved.transitioning {
            return Some(saved.frame.clone());
        }
        // A manual resize can leave the zoomed state without calling zoom:.
        for window in windows {
            frames.remove(&key(&window));
        }
        None
    })
}

pub(in crate::macos) fn remember_frame(window: &NSWindow, frame: Frame) {
    FRAMES.with(|frames| {
        frames.borrow_mut().insert(
            key(window),
            SavedFrame {
                frame,
                transitioning: false,
            },
        );
    });
}

pub(crate) struct Hook {
    window: Retained<NSWindow>,
}

impl Hook {
    pub fn install(window: Retained<NSWindow>) -> Self {
        let class = window.class();
        ORIGINAL.with(|original| {
            let mut original = original.borrow_mut();
            if let Some((installed, _)) = *original {
                assert_eq!(installed, class);
                return;
            }
            let method = class.instance_method(sel!(zoom:)).expect("NSWindow zoom:");
            let previous = method.implementation();
            // Add an override only to winit's own window class. Changing a live
            // NSWindow's class breaks AppKit's dynamic-property machinery.
            // Preserve the original encoding and IMP, including future winit
            // overrides. All installation and zoom callbacks run on the main thread.
            unsafe {
                let imp: Imp = std::mem::transmute(
                    zoom as extern "C-unwind" fn(&NSWindow, Sel, Option<&AnyObject>),
                );
                if !objc2::ffi::class_addMethod(
                    class as *const AnyClass as *mut AnyClass,
                    sel!(zoom:),
                    imp,
                    objc2::ffi::method_getTypeEncoding(method),
                )
                .as_bool()
                {
                    method.set_implementation(imp);
                }
            }
            *original = Some((class, previous));
        });
        Self { window }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        FRAMES.with(|frames| frames.borrow_mut().remove(&key(&self.window)));
    }
}
