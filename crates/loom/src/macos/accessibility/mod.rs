//! Native accessibility elements for the GPU terminal panes. Cocoa callbacks
//! use immutable snapshots; mutations return to the application event queue.
mod chrome;
mod element;
mod text;
use crate::app::App;
use crossbeam_channel::Sender;
use loom_protocol::message::ClientMessage;
use objc2::rc::Retained;
use objc2::runtime::ClassBuilder;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;
use winit::window::WindowId;

#[derive(Clone, Debug)]
pub(super) enum Action {
    Chrome(chrome::Request),
    Focus {
        key: u64,
    },
    Select {
        key: u64,
        revision: u64,
        range: NSRange,
    },
}
struct Snapshot {
    key: u64,
    revision: u64,
    window: WindowId,
    pane: u64,
    parent: Retained<NSView>,
    frame: NSRect,
    clip: NSRect,
    cell: NSSize,
    label: String,
    focused: bool,
    enabled: bool,
    secure: bool,
    text: Rc<text::Text>,
    history: bool,
    session: String,
    sender: Option<Sender<ClientMessage>>,
}
struct Entry {
    snapshot: Rc<Snapshot>,
    cache: text::Cache,
}
#[derive(Default)]
pub(super) struct Bridge {
    next_key: u64,
    views: HashMap<WindowId, Retained<NSView>>,
    entries: HashMap<(WindowId, u64), Entry>,
    deadline: Option<Instant>,
}
pub(super) fn install(class: &mut ClassBuilder) {
    element::install(class);
}
impl Bridge {
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
    pub fn begin_update(&mut self) {
        self.deadline = None;
    }
    pub fn remove(&mut self, window: WindowId) {
        self.entries.retain(|(id, _), _| *id != window);
        if let Some(view) = self.views.remove(&window) {
            chrome::remove(&view);
            element::remove(&view);
        }
    }
    pub fn update(&mut self, app: &App, view: &NSView, visible: bool, application_active: bool) {
        let Some(window) = &app.window else {
            return;
        };
        let Some(native) = super::native::native_window(window) else {
            return;
        };
        let now = Instant::now();
        let window_id = window.id();
        self.views
            .entry(window_id)
            .or_insert_with(|| Retained::from(view));
        let modal = app.modal_captures_keyboard() || app.core.overview.active;
        if !visible || modal {
            self.entries.retain(|(id, _), _| *id != window_id);
            element::publish(
                view,
                if modal {
                    "Terminal dialog or overview is open. Press Escape to return to terminal output.".into()
                } else {
                    "Terminal panes".into()
                },
                Vec::new(),
            );
            chrome::update(
                app,
                view,
                visible,
                application_active && app.window_focused && native.isKeyWindow(),
            );
            return;
        }
        chrome::update(
            app,
            view,
            visible,
            application_active && app.window_focused && native.isKeyWindow(),
        );
        let scale = app.dpi_scale.max(0.1);
        let (cw, ch) = app.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return;
        }
        let inset = app.core.config.appearance.border_width + app.core.config.appearance.padding;
        let ox = app.content_origin_x();
        let oy = app.content_origin_y();
        let content_w = app.core.workspaces.view_size.width;
        let content_h = app.core.workspaces.view_size.height;
        let tiles = app.core.workspaces.visible_tiles_2d(
            app.core.anim_mgr.view_offset_x.value() as f32,
            app.core.anim_mgr.view_offset_y.value() as f32,
        );
        let mut seen = HashSet::new();
        let mut panes = Vec::new();
        for (pane, rect, active) in tiles {
            let Some(grid) = app.core.pane_grids.get(&pane) else {
                continue;
            };
            let x = rect.x + inset;
            let y = rect.y + inset;
            let right = (rect.x + rect.w - inset).min(content_w);
            let bottom = (rect.y + rect.h - inset).min(content_h);
            let left = x.max(0.0);
            let top = y.max(0.0);
            if right <= left || bottom <= top {
                continue;
            }
            let to_screen = |x: f32, y: f32, w: f32, h: f32| {
                let rect = NSRect::new(
                    NSPoint::new((x + ox) as f64 / scale, (y + oy) as f64 / scale),
                    NSSize::new(w as f64 / scale, h as f64 / scale),
                );
                native.convertRectToScreen(view.convertRect_toView(rect, None))
            };
            let frame = to_screen(x, y, grid.cols as f32 * cw, grid.rows as f32 * ch);
            let clip = to_screen(left, top, right - left, bottom - top);
            let selection = app
                .core
                .selection
                .as_ref()
                .filter(|s| s.pane_id == pane)
                .map(|s| (s.start, s.end));
            let key = (window_id, pane);
            let identity_changed = self.entries.get(&key).is_some_and(|entry| {
                entry.snapshot.session != app.core.session_name
                    || match (&entry.snapshot.sender, &app.core.server_tx) {
                        (Some(a), Some(b)) => !a.same_channel(b),
                        (None, None) => false,
                        _ => true,
                    }
            });
            if identity_changed {
                self.entries.remove(&key);
            }
            let history = self
                .entries
                .get(&key)
                .is_some_and(|entry| element::wants_history(entry.snapshot.key));
            let previous = self.entries.remove(&key);
            let (id, mut cache) = if let Some(entry) = previous {
                (entry.snapshot.key, entry.cache)
            } else {
                self.next_key += 1;
                (
                    self.next_key,
                    text::Cache::new(grid, selection, history, now),
                )
            };
            cache.update(grid, selection, history, now);
            if let Some(deadline) = cache.deadline() {
                self.deadline = Some(self.deadline.map_or(deadline, |old| old.min(deadline)));
            }
            let label = if grid.password_input {
                format!("{}, password input", app.core.session_name)
            } else if grid.title.is_empty() {
                format!("{}, terminal {}", app.core.session_name, pane)
            } else {
                format!("{}, {}", app.core.session_name, grid.title)
            };
            let snapshot = Rc::new(Snapshot {
                key: id,
                revision: cache.revision,
                window: window_id,
                pane,
                parent: Retained::from(view),
                frame,
                clip,
                cell: NSSize::new(cw as f64 / scale, ch as f64 / scale),
                label,
                focused: application_active && active && app.window_focused && native.isKeyWindow(),
                enabled: app.core.connected && app.core.pending_session_name.is_none(),
                secure: grid.password_input,
                text: cache.text.clone(),
                history,
                session: app.core.session_name.clone(),
                sender: app.core.server_tx.clone(),
            });
            self.entries.insert(
                key,
                Entry {
                    snapshot: snapshot.clone(),
                    cache,
                },
            );
            seen.insert(key);
            panes.push(snapshot);
        }
        self.entries
            .retain(|key, _| key.0 != window_id || seen.contains(key));
        element::publish(view, "Terminal panes".into(), panes);
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        for view in self.views.values() {
            chrome::remove(view);
            element::remove(view);
        }
    }
}
impl super::MacApplication {
    pub(super) fn handle_accessibility(&mut self, action: Action) {
        if let Action::Chrome(request) = action {
            self.handle_chrome_accessibility(request);
            return;
        }
        let key = match action {
            Action::Focus { key } | Action::Select { key, .. } => key,
            Action::Chrome(_) => unreachable!(),
        };
        let Some(snapshot) = element::snapshot(key) else {
            return;
        };
        let Some(index) = self.windows.iter().position(|app| {
            app.window
                .as_ref()
                .is_some_and(|window| window.id() == snapshot.window)
        }) else {
            return;
        };
        let app = &mut self.windows[index];
        if app.modal_captures_keyboard()
            || app.core.overview.active
            || !app.core.connected
            || app.core.pending_session_name.is_some()
            || app.core.session_name != snapshot.session
        {
            return;
        }
        if !app
            .core
            .server_tx
            .as_ref()
            .zip(snapshot.sender.as_ref())
            .is_some_and(|(a, b)| a.same_channel(b))
        {
            return;
        }
        let Some(workspace) = app
            .core
            .workspaces
            .workspaces
            .iter()
            .position(|ws| ws.all_pane_ids().contains(&snapshot.pane))
        else {
            return;
        };
        let mut selection_update = None;
        if let Action::Select {
            revision, range, ..
        } = action
        {
            if snapshot.secure || revision != snapshot.revision {
                return;
            }
            let Some(grid) = app.core.pane_grids.get(&snapshot.pane) else {
                return;
            };
            let fresh = text::Text::capture(grid, None, snapshot.history);
            // A callback's range must never select different text after output
            // scrolls the buffer or a resize reflows its lines.
            if fresh.utf16 != snapshot.text.utf16 || fresh.lines != snapshot.text.lines {
                return;
            }
            let Some(range) = snapshot.text.checked_range(range.location, range.length) else {
                return;
            };
            selection_update = Some(snapshot.text.selection_cells(range).map(|(start, end)| {
                crate::app::Selection {
                    pane_id: snapshot.pane,
                    start,
                    end,
                    active: false,
                }
            }));
        }
        if !app.core.server_tx.as_ref().is_some_and(|sender| {
            sender
                .try_send(ClientMessage::FocusPane {
                    pane_id: snapshot.pane,
                })
                .is_ok()
        }) {
            return;
        }
        if let Some(selection) = selection_update {
            if let Some(selection) = &selection
                && let Some(grid) = app.core.pane_grids.get_mut(&snapshot.pane)
                && grid.buffer_to_viewport_row(selection.start.1).is_none()
            {
                grid.set_scroll_offset(
                    grid.buffer_len()
                        .saturating_sub(grid.rows as usize)
                        .saturating_sub(selection.start.1),
                );
            }
            app.core.selection = selection;
        }
        app.focus_workspace_pane_local(workspace, snapshot.pane);
        app.animate_to_active();
        app.schedule_redraw();
        if let Some(window) = app
            .window
            .as_ref()
            .and_then(|window| super::native::native_window(window))
        {
            window.makeKeyAndOrderFront(None);
            let mtm =
                objc2::MainThreadMarker::new().expect("accessibility runs on the main thread");
            #[allow(deprecated)]
            objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
        self.active = Some(snapshot.window);
    }
}
