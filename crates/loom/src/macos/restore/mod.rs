mod store;
mod zoom;
pub use store::{Frame, Group, Remote, Snapshot, Store, Tab};
pub(crate) use zoom::Hook as ZoomHook;
pub(super) use zoom::remember_frame;

use super::native;
use crate::app::App;
use objc2_app_kit::NSWindowStyleMask;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::collections::{HashMap, HashSet};
use winit::window::WindowId;

impl From<NSRect> for Frame {
    fn from(frame: NSRect) -> Self {
        Frame([
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ])
    }
}
impl From<&Frame> for NSRect {
    fn from(frame: &Frame) -> Self {
        NSRect::new(
            NSPoint::new(frame.0[0], frame.0[1]),
            NSSize::new(frame.0[2], frame.0[3]),
        )
    }
}

pub fn capture(
    apps: &[App],
    active: Option<WindowId>,
    normal_frames: &mut HashMap<WindowId, Frame>,
) -> Snapshot {
    let windows: Vec<_> = apps
        .iter()
        .filter(|app| !app.native_quick_terminal)
        .filter_map(|app| {
            let window = app.window.as_ref()?;
            Some((app, window, native::native_window(window)?))
        })
        .collect();
    for (_, window, native) in &windows {
        if let Some(frame) = zoom::normal_frame(native) {
            normal_frames.insert(window.id(), frame);
        } else if window.fullscreen().is_none()
            && !native.styleMask().contains(NSWindowStyleMask::FullScreen)
            && !native.isZoomed()
            && !native.isMiniaturized()
        {
            normal_frames.insert(window.id(), Frame::from(native.frame()));
        }
    }
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    let mut active_group = None;
    for (_, window, native) in &windows {
        if seen.contains(&window.id()) {
            continue;
        }
        let group = native.tabGroup();
        let tabs: Vec<_> = group
            .as_ref()
            .map(|group| group.windows())
            .into_iter()
            .flat_map(|tabs| tabs.to_vec())
            .filter_map(|tab| windows.iter().find(|(_, _, candidate)| **candidate == *tab))
            .collect();
        let tabs = if tabs.is_empty() {
            vec![
                windows
                    .iter()
                    .find(|(_, candidate, _)| candidate.id() == window.id())
                    .unwrap(),
            ]
        } else {
            tabs
        };
        let selected_window = group.as_ref().and_then(|group| group.selectedWindow());
        let selected = tabs
            .iter()
            .position(|(_, _, native)| selected_window.as_ref() == Some(native))
            .unwrap_or(0);
        let (_, selected_window, selected_native) = tabs[selected];
        let frame = normal_frames
            .get(&selected_window.id())
            .cloned()
            .unwrap_or_else(|| Frame::from(selected_native.frame()));
        if tabs
            .iter()
            .any(|(_, window, _)| active == Some(window.id()))
        {
            active_group = Some(groups.len());
        }
        let saved_tabs = tabs
            .iter()
            .map(|(app, window, _)| {
                seen.insert(window.id());
                Tab {
                    session: app.core.session_name.clone(),
                    remote: app.core.remote_config.as_ref().map(|remote| Remote {
                        host: remote.host.clone(),
                        port: remote.port,
                        ssh_port: remote.ssh_port,
                    }),
                }
            })
            .collect();
        groups.push(Group {
            frame,
            tabs: saved_tabs,
            selected,
            minimized: selected_native.isMiniaturized(),
            zoomed: selected_native.isZoomed(),
            fullscreen: selected_window.fullscreen().is_some()
                || selected_native
                    .styleMask()
                    .contains(NSWindowStyleMask::FullScreen),
        });
    }
    Snapshot {
        version: 1,
        groups,
        active_group,
    }
}
