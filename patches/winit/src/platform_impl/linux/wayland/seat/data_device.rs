//! Wayland data device (drag-and-drop) handling.

use std::io::Read;
use std::path::PathBuf;

use sctk::data_device_manager::data_device::{DataDeviceData, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::DataSourceHandler;
use sctk::data_device_manager::WritePipe;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use crate::event::WindowEvent;
use crate::platform_impl::wayland::state::WinitState;

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        wl_surface: &WlSurface,
    ) {
        let data = match data_device.data::<DataDeviceData>() {
            Some(data) => data,
            None => return,
        };
        let drag_offer = match data.drag_offer() {
            Some(offer) => offer,
            None => return,
        };

        // Check if the offer has text/uri-list
        let has_uri_list: bool = drag_offer.with_mime_types(|mime_types: &[String]| {
            mime_types.iter().any(|m| m == "text/uri-list")
        });

        if !has_uri_list {
            return;
        }

        // Accept the offer
        drag_offer.accept_mime_type(drag_offer.serial, Some("text/uri-list".to_string()));
        drag_offer.set_actions(DndAction::Copy, DndAction::Copy);

        // Store the window being hovered for later events
        let window_id = super::super::make_wid(wl_surface);
        self.dnd_offer_window = Some(window_id);

        self.dispatched_events = true;
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        if let Some(window_id) = self.dnd_offer_window.take() {
            self.events_sink.push_window_event(WindowEvent::HoveredFileCancelled, window_id);
            self.dnd_offer_paths.clear();
            self.dispatched_events = true;
        }
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
        // Motion events during DnD. The Wayland protocol doesn't give us
        // the file list during motion, so we can't emit per-file HoveredFile
        // events here.
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        // Clipboard selection changed - not relevant for DnD.
    }

    fn drop_performed(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let window_id = match self.dnd_offer_window {
            Some(id) => id,
            None => return,
        };

        let data = match data_device.data::<DataDeviceData>() {
            Some(data) => data,
            None => return,
        };
        let drag_offer = match data.drag_offer() {
            Some(offer) => offer,
            None => return,
        };

        // Request the data
        let read_pipe: sctk::data_device_manager::ReadPipe =
            match drag_offer.receive("text/uri-list".to_string()) {
                Ok(pipe) => pipe,
                Err(err) => {
                    tracing::warn!("Failed to receive DnD data: {err}");
                    return;
                },
            };

        // We must flush the connection before reading from the pipe to avoid deadlock.
        if let Err(err) = conn.flush() {
            tracing::warn!("Failed to flush Wayland connection: {err}");
            return;
        }

        // Convert ReadPipe to OwnedFd, then to File for reading
        let owned_fd: std::os::unix::io::OwnedFd = read_pipe.into();
        let mut file = std::fs::File::from(owned_fd);

        let mut data_buf = String::new();
        if let Err(err) = file.read_to_string(&mut data_buf) {
            tracing::warn!("Failed to read DnD data: {err}");
            return;
        }

        let paths = parse_uri_list(&data_buf);

        // Emit HoveredFile for each path first (so downstream can see what was dropped)
        for path in &paths {
            self.events_sink
                .push_window_event(WindowEvent::HoveredFile(path.clone()), window_id);
        }

        // Emit DroppedFile for each path
        for path in &paths {
            self.events_sink
                .push_window_event(WindowEvent::DroppedFile(path.clone()), window_id);
        }

        // Finish the offer
        drag_offer.finish();

        self.dnd_offer_paths.clear();
        self.dnd_offer_window = None;
        self.dispatched_events = true;
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: DndAction,
    ) {
        // We always accept Copy action, no special handling needed.
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: DndAction,
    ) {
        // No special handling needed.
    }
}

impl DataSourceHandler for WinitState {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }

    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: String,
        _fd: WritePipe,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }

    fn cancelled(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }

    fn dnd_dropped(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }

    fn dnd_finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }

    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _action: DndAction,
    ) {
        // We don't initiate DnD from winit, so this is unused.
    }
}

/// Parse a `text/uri-list` payload into a list of file paths.
///
/// The format is one URI per line, separated by `\r\n`.
/// Lines starting with `#` are comments.
/// Only `file://` URIs are supported; percent-encoded characters are decoded.
pub(crate) fn parse_uri_list(data: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in data.split('\n') {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(path_str) = line.strip_prefix("file://") {
            // Handle file:///path (empty host) and file://localhost/path
            let path_str = if path_str.starts_with('/') {
                path_str
            } else {
                // Has hostname like file://hostname/path — skip to the path part
                match path_str.find('/') {
                    Some(idx) => &path_str[idx..],
                    None => continue,
                }
            };
            // Percent-decode the path (no canonicalize — avoids blocking I/O in event loop)
            let decoded = percent_encoding::percent_decode_str(path_str)
                .decode_utf8_lossy()
                .into_owned();
            paths.push(PathBuf::from(decoded));
        }
    }
    paths
}
