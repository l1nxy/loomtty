//! An on-demand terminal which keeps its connection while hidden. Its window
//! is never a member of a normal tab group or a candidate for window restore.
mod geometry;
pub mod hotkey;
pub mod session;

use std::time::{Duration, Instant};

use loom_config::{MacosQuickTerminalConfig, QuickTerminalScreen};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSEvent, NSRunningApplication, NSScreen,
    NSWindowCollectionBehavior, NSWindowTabbingMode, NSWorkspace,
};
use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo, NSRect};
use winit::window::{Window, WindowAttributes, WindowId};

use super::native::native_window;

fn selected_screen(config: &MacosQuickTerminalConfig) -> Option<Retained<NSScreen>> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    if config.screen == QuickTerminalScreen::Mouse {
        let pointer = NSEvent::mouseLocation();
        for screen in screens.iter() {
            let frame = screen.frame();
            if pointer.x >= frame.origin.x
                && pointer.y >= frame.origin.y
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y < frame.origin.y + frame.size.height
            {
                return Some(screen);
            }
        }
    }
    // The first screen owns the menu bar; NSScreen.mainScreen is instead the
    // screen containing the key window, which may belong to another display.
    screens.firstObject()
}

fn usable_frame(screen: &NSScreen) -> NSRect {
    let visible = screen.visibleFrame();
    if !NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 12,
        minorVersion: 0,
        patchVersion: 0,
    }) {
        return visible;
    }
    // visibleFrame alone can include the camera housing with an auto-hidden
    // menu bar. Intersect with the display's safe area on notched MacBooks.
    let screen_frame = screen.frame();
    let safe = screen.safeAreaInsets();
    let x = visible.origin.x.max(screen_frame.origin.x + safe.left);
    let y = visible.origin.y.max(screen_frame.origin.y + safe.bottom);
    let right = (visible.origin.x + visible.size.width)
        .min(screen_frame.origin.x + screen_frame.size.width - safe.right);
    let top = (visible.origin.y + visible.size.height)
        .min(screen_frame.origin.y + screen_frame.size.height - safe.top);
    NSRect::new(
        objc2_foundation::NSPoint::new(x, y),
        objc2_foundation::NSSize::new((right - x).max(1.0), (top - y).max(1.0)),
    )
}

pub fn attributes(attrs: WindowAttributes, config: &MacosQuickTerminalConfig) -> WindowAttributes {
    let attrs = attrs
        .with_visible(false)
        .with_active(false)
        .with_decorations(false)
        .with_resizable(false);
    if let Some(screen) = selected_screen(config) {
        let frame = geometry::target_frame(usable_frame(&screen), config);
        attrs.with_inner_size(winit::dpi::LogicalSize::new(
            frame.size.width,
            frame.size.height,
        ))
    } else {
        attrs
    }
}

pub fn configure_window(window: &Window) {
    if let Some(window) = native_window(window) {
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        window.setExcludedFromWindowsMenu(true);
        window.setLevel(objc2_app_kit::NSStatusWindowLevel);
        let mut behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle;
        if NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
            majorVersion: 13,
            minorVersion: 0,
            patchVersion: 0,
        }) {
            behavior |= NSWindowCollectionBehavior::CanJoinAllApplications;
        }
        window.setCollectionBehavior(behavior);
        // We own the hide animation and focus restoration instead of letting
        // AppKit immediately order the window out when the app deactivates.
        window.setHidesOnDeactivate(false);
    }
}

pub struct QuickTerminal {
    pub id: WindowId,
    pub previous_window: Option<WindowId>,
    previous_application: Option<Retained<NSRunningApplication>>,
    restore_focus: bool,
    pub visible: bool,
    ordered_in: bool,
    slide: geometry::Slide,
    target: NSRect,
    edge: loom_config::QuickTerminalPosition,
    fullscreen: bool,
    resize_pending: bool,
}

impl QuickTerminal {
    pub fn new(window: &Window, config: &MacosQuickTerminalConfig) -> Self {
        Self {
            id: window.id(),
            previous_window: None,
            previous_application: None,
            restore_focus: false,
            visible: false,
            ordered_in: false,
            slide: geometry::Slide::hidden(Instant::now()),
            target: native_window(window).map(|w| w.frame()).unwrap_or_default(),
            edge: config.position,
            fullscreen: false,
            resize_pending: false,
        }
    }

    /// Capture before constructing or showing the Quick Terminal, so creating
    /// its renderer/window cannot change the application we return to.
    pub fn frontmost_application() -> Option<Retained<NSRunningApplication>> {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        (application.processIdentifier() != std::process::id() as i32).then_some(application)
    }

    pub fn show(
        &mut self,
        window: &Window,
        config: &MacosQuickTerminalConfig,
        previous_window: Option<WindowId>,
        previous_application: Option<Retained<NSRunningApplication>>,
    ) {
        // During a reversal, preserve the original app/window to return to.
        if !self.ordered_in {
            self.previous_window = previous_window.filter(|id| *id != self.id);
            self.previous_application = previous_application;
        }
        if let Some(screen) = selected_screen(config) {
            self.target = if self.fullscreen {
                usable_frame(&screen)
            } else {
                geometry::target_frame(usable_frame(&screen), config)
            };
        }
        self.edge = config.position;
        self.visible = true;
        self.resize_pending = true;
        self.restore_focus = false;
        self.slide
            .retarget(true, animation_duration(config), Instant::now());
        if let Some(native) = native_window(window) {
            native.setFrame_display(
                geometry::revealed_frame(self.target, self.edge, self.slide.value(Instant::now())),
                false,
            );
            native.makeKeyAndOrderFront(None);
            native.makeMainWindow();
            // No ActivateAllWindows: unrelated loom windows stay where they are.
            #[allow(deprecated)]
            NSRunningApplication::currentApplication()
                .activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        }
        self.ordered_in = true;
        window.set_ime_allowed(true);
        window.request_redraw();
    }

    pub fn hide(
        &mut self,
        window: &Window,
        config: &MacosQuickTerminalConfig,
        restore_focus: bool,
    ) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.restore_focus = restore_focus;
        self.slide
            .retarget(false, animation_duration(config), Instant::now());
        window.set_ime_allowed(false);
    }

    /// Returns the normal loom window to refocus after an explicit dismissal.
    /// A focus-loss dismissal must never steal focus back from the clicked app.
    pub fn step(&mut self, window: &Window, now: Instant) -> Option<WindowId> {
        if !self.ordered_in {
            return None;
        }
        if let Some(native) = native_window(window) {
            let progress = self.slide.value(now);
            if !self.visible && !self.slide.animating(now) {
                let mtm = MainThreadMarker::new().expect("macOS main thread");
                let can_restore =
                    self.restore_focus && NSApplication::sharedApplication(mtm).isActive();
                native.orderOut(None);
                // Keep the backing view at its full size while hidden. Shells
                // must not reflow into a one-row terminal after every toggle.
                native.setFrame_display(self.target, false);
                self.ordered_in = false;
                self.resize_pending = false;
                if can_restore {
                    if let Some(app) = self.previous_application.take() {
                        #[allow(deprecated)]
                        app.activateWithOptions(
                            NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                        );
                    } else {
                        return self.previous_window.take();
                    }
                }
            } else {
                let frame = geometry::revealed_frame(self.target, self.edge, progress);
                if native.frame() != frame {
                    native.setFrame_display(frame, true);
                }
            }
        }
        None
    }

    pub fn defer_resize(&self, now: Instant) -> bool {
        !self.visible || self.slide.animating(now)
    }

    pub fn take_final_resize(&mut self, now: Instant) -> bool {
        if self.resize_pending && !self.defer_resize(now) {
            self.resize_pending = false;
            true
        } else {
            false
        }
    }

    pub fn toggle_fullscreen(&mut self, window: &Window, config: &MacosQuickTerminalConfig) {
        self.fullscreen = !self.fullscreen;
        if let Some(screen) = selected_screen(config) {
            self.target = if self.fullscreen {
                usable_frame(&screen)
            } else {
                geometry::target_frame(usable_frame(&screen), config)
            };
            self.slide.retarget(true, Duration::ZERO, Instant::now());
            self.resize_pending = true;
            if let Some(window) = native_window(window) {
                window.setFrame_display(self.target, true);
            }
        }
    }

    pub fn next_frame(&self, now: Instant) -> Option<Instant> {
        (self.ordered_in && self.slide.animating(now)).then_some(now + Duration::from_millis(8))
    }
}

fn animation_duration(config: &MacosQuickTerminalConfig) -> Duration {
    if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
        Duration::ZERO
    } else {
        Duration::from_millis(config.animation_ms)
    }
}
