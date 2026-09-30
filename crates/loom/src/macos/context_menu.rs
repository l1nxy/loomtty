//! AppKit terminal context menus, presented outside winit's borrowed callbacks.
use std::cell::RefCell;
use std::collections::HashMap;

use crossbeam_channel::Sender;
use loom_protocol::message::ClientMessage;
use muda::{ContextMenu as _, Menu, MenuId, MenuItem, PredefinedMenuItem};
use objc2::rc::Retained;
use objc2_app_kit::{NSMenu, NSView};
use objc2_foundation::{NSPoint, NSRect};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::WindowId;

use crate::app::{App, ContextMenuAction, ContextMenuItem};

#[derive(Clone, Debug)]
struct Target {
    window: WindowId,
    pane: u64,
    session: String,
    sender: Option<Sender<ClientMessage>>,
}
impl Target {
    fn same_connection(&self, session: &str, sender: Option<&Sender<ClientMessage>>) -> bool {
        self.session == session
            && match (self.sender.as_ref(), sender) {
                (Some(a), Some(b)) => a.same_channel(b),
                (None, None) => true,
                _ => false,
            }
    }

    fn matches(&self, app: &App) -> bool {
        app.window.as_ref().is_some_and(|w| w.id() == self.window)
            && self.same_connection(&app.core.session_name, app.core.server_tx.as_ref())
            && app.core.pending_session_name.is_none()
            && app
                .core
                .workspaces
                .active()
                .all_pane_ids()
                .contains(&self.pane)
            && app.core.pane_grids.contains_key(&self.pane)
            && !app.modal_captures_keyboard()
            && !app.core.overview.active
    }
}

#[derive(Clone, Debug)]
pub(super) struct Request {
    target: Target,
    item: ContextMenuItem,
}
struct Pending {
    view: Retained<NSView>,
    target: Target,
    items: Vec<ContextMenuItem>,
    point: NSPoint,
}
struct Active {
    window: WindowId,
    menu: Menu,
    commands: HashMap<MenuId, Request>,
}
thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

fn view_point(x: f64, y: f64, scale: f64, bounds: NSRect, flipped: bool) -> NSPoint {
    NSPoint::new(
        bounds.origin.x + x / scale,
        bounds.origin.y
            + if flipped {
                y / scale
            } else {
                bounds.size.height - y / scale
            },
    )
}

fn allowed(item: &ContextMenuItem, connected: bool, password: bool) -> bool {
    item.enabled
        && match item.action {
            ContextMenuAction::Copy
            | ContextMenuAction::SelectAll
            | ContextMenuAction::Search
            | ContextMenuAction::OpenLink(_)
            | ContextMenuAction::CopyLink(_) => !password,
            ContextMenuAction::Paste
            | ContextMenuAction::SplitRight
            | ContextMenuAction::SplitDown
            | ContextMenuAction::ClosePane => connected,
            _ => false,
        }
}

pub(crate) fn open(app: &App) {
    let Some(window) = &app.window else { return };
    let Some(pane) = app.core.context_menu.target_pane_id.or(app
        .core
        .workspaces
        .active()
        .active_pane_id())
    else {
        return;
    };
    let Some(grid) = app.core.pane_grids.get(&pane) else {
        return;
    };
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit owns this live view. Retention keeps it valid while the
    // queued popup runs; its window is checked again before presentation.
    let Some(view) = (unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }) else {
        return;
    };
    let point = view_point(
        app.core.context_menu.x.into(),
        app.core.context_menu.y.into(),
        window.scale_factor(),
        view.bounds(),
        view.isFlipped(),
    );
    let mut items = app.core.context_menu.items.clone();
    for item in &mut items {
        item.enabled = allowed(item, app.core.connected, grid.password_input);
        if matches!(item.action, ContextMenuAction::Copy) {
            item.enabled &= app
                .core
                .selection
                .as_ref()
                .is_some_and(|selection| selection.pane_id == pane);
        }
    }
    if ACTIVE.with(|active| active.borrow().is_some()) {
        return;
    }
    PENDING.with(|pending| {
        *pending.borrow_mut() = Some(Pending {
            view,
            point,
            items,
            target: Target {
                window: window.id(),
                pane,
                session: app.core.session_name.clone(),
                sender: app.core.server_tx.clone(),
            },
        })
    });
    // NSMenu tracks a nested event loop. winit must have released the mutable
    // application callback so redraws and server messages can keep running.
    dispatch2::DispatchQueue::main().exec_async(present);
}

fn present() {
    let Some(pending) = PENDING.with(|pending| pending.borrow_mut().take()) else {
        return;
    };
    if !pending
        .view
        .window()
        .is_some_and(|window| window.isVisible())
    {
        return;
    }
    let menu = Menu::new();
    let mut commands = HashMap::new();
    for item in pending.items {
        let result = if item.label.chars().all(|ch| ch == '\u{2500}') {
            menu.append(&PredefinedMenuItem::separator())
        } else {
            let entry = MenuItem::new(&item.label, item.enabled, None);
            commands.insert(
                entry.id().clone(),
                Request {
                    target: pending.target.clone(),
                    item,
                },
            );
            menu.append(&entry)
        };
        if let Err(error) = result {
            log::warn!("could not build native context menu: {error}");
            return;
        }
    }
    ACTIVE.with(|active| {
        *active.borrow_mut() = Some(Active {
            window: pending.target.window,
            menu: menu.clone(),
            commands,
        })
    });
    // SAFETY: muda retains this NSMenu for the lifetime of `menu`. Use AppKit
    // directly because winit's view is flipped; muda's position helper assumes
    // a bottom-left origin. No Rust/TLS borrow crosses the tracking loop.
    let native = unsafe { &*menu.ns_menu().cast::<NSMenu>() };
    native.popUpMenuPositioningItem_atLocation_inView(None, pending.point, Some(&pending.view));
    ACTIVE.with(|active| active.borrow_mut().take());
}

// Resolve before the popup is dropped, at the native callback boundary. The
// queued command owns both its action and its original terminal identity.
pub(super) fn command(id: &MenuId) -> Option<Request> {
    ACTIVE.with(|active| active.borrow().as_ref()?.commands.get(id).cloned())
}

pub(super) fn cancel(window: WindowId) {
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.as_ref().is_some_and(|p| p.target.window == window) {
            pending.take();
        }
    });
    let menu = ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .filter(|active| active.window == window)
            .map(|active| active.menu.clone())
    });
    if let Some(menu) = menu {
        // SAFETY: the menu clone owns the NSMenu through cancellation.
        unsafe { &*menu.ns_menu().cast::<NSMenu>() }.cancelTracking();
    }
}

impl super::MacApplication {
    pub(super) fn handle_context_menu(&mut self, request: Request) {
        let Some(app) = self
            .windows
            .iter_mut()
            .find(|app| request.target.matches(app))
        else {
            return;
        };
        let password = app.core.pane_grids[&request.target.pane].password_input;
        if !allowed(&request.item, app.core.connected, password) {
            return;
        }
        // Selection may change while AppKit runs its nested tracking loop.
        // Never copy another pane's text through a previously enabled item.
        if matches!(request.item.action, ContextMenuAction::Copy)
            && !app
                .core
                .selection
                .as_ref()
                .is_some_and(|selection| selection.pane_id == request.target.pane)
        {
            return;
        }
        match request.item.action {
            ContextMenuAction::SplitRight | ContextMenuAction::SplitDown => {
                // The server focuses and splits atomically, even if another
                // client changes focus while this native menu is open.
                app.send(ClientMessage::CreatePaneAt {
                    pane_id: request.target.pane,
                    below: matches!(request.item.action, ContextMenuAction::SplitDown),
                    request_id: 0,
                });
            }
            _ => app.execute_context_menu_item(Some(request.target.pane), request.item),
        }
        app.schedule_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSSize;

    #[test]
    fn popup_coordinates_follow_retina_scale_and_view_orientation() {
        let bounds = NSRect::new(NSPoint::new(3.0, 7.0), NSSize::new(600.0, 400.0));
        assert_eq!(
            view_point(200.0, 120.0, 2.0, bounds, true),
            NSPoint::new(103.0, 67.0)
        );
        assert_eq!(
            view_point(200.0, 120.0, 2.0, bounds, false),
            NSPoint::new(103.0, 347.0)
        );
        assert_eq!(
            view_point(100.0, 60.0, 1.0, bounds, true),
            NSPoint::new(103.0, 67.0)
        );
    }

    #[test]
    fn queued_context_action_does_not_follow_session_or_connection_replacement() {
        let (sender, _) = crossbeam_channel::unbounded();
        let target = Target {
            window: WindowId::from(1),
            pane: 42,
            session: "one".into(),
            sender: Some(sender.clone()),
        };
        assert!(target.same_connection("one", Some(&sender)));
        assert!(!target.same_connection("two", Some(&sender)));
        assert!(!target.same_connection("one", Some(&crossbeam_channel::unbounded().0)));
        assert!(!target.same_connection("one", None));
    }

    #[test]
    fn menu_permissions_are_rechecked_after_password_or_connection_changes() {
        let item = |action| ContextMenuItem {
            label: String::new(),
            action,
            enabled: true,
        };
        assert!(allowed(&item(ContextMenuAction::Copy), false, false));
        assert!(!allowed(&item(ContextMenuAction::Copy), true, true));
        assert!(!allowed(&item(ContextMenuAction::Paste), false, false));
        assert!(allowed(&item(ContextMenuAction::Paste), true, true));
        assert!(!allowed(&item(ContextMenuAction::SplitRight), false, false));
    }
}
