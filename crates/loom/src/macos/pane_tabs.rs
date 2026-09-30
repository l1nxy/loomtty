//! Native pane navigation inside one session. AppKit window tabs are a
//! separate feature; these controls never create a session or an NSWindow.
mod layout;

use std::cell::RefCell;
use std::ops::Range;

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSFont, NSFontAttributeName,
    NSLayoutAttribute, NSLineBreakMode, NSScrollElasticity, NSScrollView, NSScrollerStyle,
    NSSegmentDistribution, NSSegmentStyle, NSSegmentSwitchTracking, NSSegmentedControl,
    NSStringDrawing, NSTitlebarAccessoryViewController, NSView, NSViewBoundsDidChangeNotification,
};
use objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString,
};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use super::{Command, native};
use crate::app::App;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Focus(u64),
    New,
    Close(u64),
}

#[derive(Clone, Debug)]
pub(super) struct Request {
    pub window: WindowId,
    pub session: String,
    pub operation: Operation,
}

impl Request {
    pub fn matches(&self, app: &App) -> bool {
        app.window.as_ref().is_some_and(|w| w.id() == self.window)
            && app.core.session_name == self.session
            && app.core.pending_session_name.is_none()
            && app.core.connected
            && !app.modal_captures_keyboard()
            && match self.operation {
                Operation::New => true,
                Operation::Focus(pane) | Operation::Close(pane) => {
                    app.pane_tab_entries().iter().any(|(id, _)| *id == pane)
                }
            }
    }
}

struct TargetState {
    requests: RefCell<Vec<Request>>,
    sender: Sender<Command>,
    proxy: EventLoopProxy<()>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetState]
    struct PaneTabTarget;
    unsafe impl NSObjectProtocol for PaneTabTarget {}
    impl PaneTabTarget {
        #[unsafe(method(selectPane:))]
        fn select(&self, control: &NSSegmentedControl) {
            let index = control.selectedSegment();
            let request = usize::try_from(index).ok()
                .and_then(|index| self.ivars().requests.borrow().get(index).cloned());
            self.queue(request);
        }
        #[unsafe(method(newPane:))]
        fn new_pane(&self, _button: &NSButton) {
            self.queue(self.ivars().requests.borrow().iter()
                .find(|request| request.operation == Operation::New).cloned());
        }
        #[unsafe(method(closePane:))]
        fn close_pane(&self, _button: &NSButton) {
            self.queue(self.ivars().requests.borrow().iter()
                .find(|request| matches!(request.operation, Operation::Close(_))).cloned());
        }
        #[unsafe(method(viewportDidChange:))]
        fn viewport_changed(&self, _notification: &NSNotification) {
            let _ = self.ivars().proxy.send_event(());
        }
    }
);

impl PaneTabTarget {
    fn queue(&self, request: Option<Request>) {
        if let Some(request) = request {
            let _ = self.ivars().sender.send(Command::PaneTab(request));
            let _ = self.ivars().proxy.send_event(());
        }
    }
}

pub(super) struct PaneTabs {
    _accessory: Retained<NSTitlebarAccessoryViewController>,
    scroll: Retained<NSScrollView>,
    document: Retained<NSView>,
    control: Retained<NSSegmentedControl>,
    new_button: Retained<NSButton>,
    close_button: Retained<NSButton>,
    target: Retained<PaneTabTarget>,
    entries: Vec<(u64, String)>,
    active: Option<u64>,
    session: String,
    labels: Vec<String>,
    measured_widths: Vec<f64>,
    layout: layout::Layout,
    rendered: Range<usize>,
    viewport_size: NSSize,
}

impl Drop for PaneTabs {
    fn drop(&mut self) {
        // NSWindow may retain the accessory after this Rust owner goes away.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(&self.target);
            self.control.setTarget(None);
            self.new_button.setTarget(None);
            self.close_button.setTarget(None);
        }
    }
}

impl PaneTabs {
    pub fn new(app: &App, sender: Sender<Command>, proxy: EventLoopProxy<()>) -> Option<Self> {
        let window = native::native_window(app.window.as_ref()?)?;
        let mtm = MainThreadMarker::new()?;
        let target: Retained<PaneTabTarget> = unsafe {
            msg_send![
                super(PaneTabTarget::alloc(mtm).set_ivars(TargetState {
                    requests: RefCell::default(),
                    sender,
                    proxy,
                })),
                init
            ]
        };
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(window.frame().size.width, 36.0));
        let root = NSView::initWithFrame(NSView::alloc(mtm), frame);
        root.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let control = NSSegmentedControl::initWithFrame(NSSegmentedControl::alloc(mtm), frame);
        control.setSegmentStyle(NSSegmentStyle::TexturedRounded);
        control.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        control.setSegmentDistribution(NSSegmentDistribution::Fit);
        control.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        unsafe {
            // The target is retained for the control's entire lifetime.
            control.setTarget(Some(&target));
            control.setAction(Some(sel!(selectPane:)));
        }
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            NSRect::new(
                NSPoint::new(4.0, 0.0),
                NSSize::new((frame.size.width - 80.0).max(1.0), 36.0),
            ),
        );
        scroll.setDrawsBackground(false);
        scroll.setHasHorizontalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setHorizontalScrollElasticity(NSScrollElasticity::None);
        scroll.setVerticalScrollElasticity(NSScrollElasticity::None);
        scroll.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let document = NSView::initWithFrame(NSView::alloc(mtm), frame);
        document.addSubview(&control);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        unsafe {
            // The observer lives until Drop; its callback only wakes the loop.
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &target,
                sel!(viewportDidChange:),
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
        }
        root.addSubview(&scroll);
        let new_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("+"),
                Some(&target),
                Some(sel!(newPane:)),
                mtm,
            )
        };
        let close_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("×"),
                Some(&target),
                Some(sel!(closePane:)),
                mtm,
            )
        };
        for (button, offset, help) in [
            (&new_button, 72.0, "New Pane (⌘T)"),
            (&close_button, 36.0, "Close Selected Pane"),
        ] {
            button.setBezelStyle(NSBezelStyle::Toolbar);
            button.setFrame(NSRect::new(
                NSPoint::new(frame.size.width - offset, 6.0),
                NSSize::new(28.0, 24.0),
            ));
            button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
            button.setToolTip(Some(&NSString::from_str(help)));
            root.addSubview(button);
        }
        let accessory = NSTitlebarAccessoryViewController::new(mtm);
        accessory.setView(&root);
        accessory.setLayoutAttribute(NSLayoutAttribute::Bottom);
        accessory.setFullScreenMinHeight(36.0);
        window.addTitlebarAccessoryViewController(&accessory);
        Some(Self {
            _accessory: accessory,
            scroll,
            document,
            control,
            new_button,
            close_button,
            target,
            entries: Vec::new(),
            active: None,
            session: String::new(),
            labels: Vec::new(),
            measured_widths: Vec::new(),
            layout: layout::Layout::default(),
            rendered: 0..0,
            viewport_size: NSSize::ZERO,
        })
    }

    pub fn update(&mut self, app: &App) {
        let Some(window) = &app.window else { return };
        let entries = app.pane_tab_entries();
        let active = app.core.workspaces.active().active_pane_id();
        let context_changed = self.session != app.core.session_name;
        let changed = self.entries != entries || context_changed;
        let selection_changed = self.active != active;
        let clip = self.scroll.contentView();
        let previous_viewport = clip.bounds();
        let active_was_visible = self
            .entries
            .iter()
            .position(|(id, _)| Some(*id) == self.active)
            .is_some_and(|index| {
                self.layout.start(index) < previous_viewport.origin.x + previous_viewport.size.width
                    && self.layout.start(index) + self.layout.width(index)
                        > previous_viewport.origin.x
            });
        if changed {
            let labels: Vec<_> = entries
                .iter()
                .enumerate()
                .map(|(index, (_, label))| format!("{}  {label}", index + 1))
                .collect();
            let font = self
                .control
                .font()
                .unwrap_or_else(|| NSFont::systemFontOfSize(NSFont::systemFontSize()));
            let attrs = NSDictionary::from_slices(
                &[unsafe { NSFontAttributeName }],
                &[&*font as &objc2::runtime::AnyObject],
            );
            // Use AppKit's actual font metrics, including Unicode fallback.
            let widths: Vec<_> = labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    if self.labels.get(index) == Some(label) {
                        self.measured_widths[index]
                    } else {
                        unsafe {
                            NSString::from_str(label)
                                .sizeWithAttributes(Some(&attrs))
                                .width
                        }
                    }
                })
                .collect();
            self.layout = layout::Layout::measured(widths.iter().copied());
            self.labels = labels;
            self.measured_widths = widths;
        }
        let viewport = self.scroll.contentSize();
        let viewport_changed = self.viewport_size != viewport;
        // "Always show scroll bars" can reduce the clip height even when
        // overlay scrollers were requested. Align actions with the actual row.
        for button in [&self.new_button, &self.close_button] {
            let frame = button.frame();
            let origin = NSPoint::new(
                frame.origin.x,
                clip.frame().origin.y + (viewport.height - frame.size.height) / 2.0,
            );
            if frame.origin != origin {
                button.setFrameOrigin(origin);
            }
        }
        let document_size = NSSize::new(self.layout.total().max(viewport.width), viewport.height);
        if self.document.frame().size != document_size {
            self.document.setFrameSize(document_size);
        }
        let selected = entries.iter().position(|(id, _)| Some(*id) == active);
        // Background title updates must not drag a user browsing other tabs
        // back to the active pane. Reveal on selection/resize, or when the
        // currently visible active label changes width.
        if (context_changed
            || selection_changed
            || viewport_changed
            || (changed && active_was_visible))
            && let Some(index) = selected
        {
            let current = clip.bounds().origin.x;
            let next = self.layout.reveal(index, current, viewport.width);
            if next != current {
                clip.scrollToPoint(NSPoint::new(next, 0.0));
                self.scroll.reflectScrolledClipView(&clip);
            }
        }
        let visible = self.layout.visible(clip.bounds().origin.x, viewport.width);
        if changed || selection_changed || viewport_changed || self.rendered != visible {
            let mut operations: Vec<_> = entries[visible.clone()]
                .iter()
                .map(|(id, _)| Operation::Focus(*id))
                .collect();
            operations.push(Operation::New);
            if let Some(pane) = active {
                operations.push(Operation::Close(pane));
            }
            *self.target.ivars().requests.borrow_mut() = operations
                .into_iter()
                .map(|operation| Request {
                    window: window.id(),
                    session: app.core.session_name.clone(),
                    operation,
                })
                .collect();
            self.control.setSegmentCount(visible.len() as isize);
            for (segment, index) in visible.clone().enumerate() {
                let label = NSString::from_str(&self.labels[index]);
                self.control.setLabel_forSegment(&label, segment as isize);
                self.control
                    .setToolTip_forSegment(Some(&label), segment as isize);
                self.control
                    .setWidth_forSegment(self.layout.width(index), segment as isize);
            }
            let start = if visible.is_empty() {
                0.0
            } else {
                self.layout.start(visible.start)
            };
            let width = visible
                .clone()
                .map(|index| self.layout.width(index))
                .sum::<f64>();
            self.control.setFrame(NSRect::new(
                NSPoint::new(start, 0.0),
                NSSize::new(width, viewport.height),
            ));
        }
        let enabled = app.core.connected
            && app.core.pending_session_name.is_none()
            && !app.modal_captures_keyboard();
        self.control.setEnabled(enabled);
        self.new_button.setEnabled(enabled);
        self.close_button.setEnabled(enabled && active.is_some());
        self.control.setSelectedSegment(
            selected
                .filter(|index| visible.contains(index))
                .map_or(-1, |index| (index - visible.start) as isize),
        );
        self.rendered = visible;
        self.viewport_size = viewport;
        self.entries = entries;
        self.active = active;
        self.session.clone_from(&app.core.session_name);
    }
}
