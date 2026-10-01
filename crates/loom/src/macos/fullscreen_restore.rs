//! AppKit cannot reliably enter two fullscreen Spaces simultaneously.
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use winit::window::{Fullscreen, WindowId};

use super::{MacApplication, native};

#[derive(Default)]
pub(super) struct Queue {
    pub pending: VecDeque<WindowId>,
    pub waiting: Option<(WindowId, usize, Instant)>,
    pub focus: Option<WindowId>,
}

impl Queue {
    pub fn active(&self) -> bool {
        self.waiting.is_some() || !self.pending.is_empty()
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.waiting.map(|(_, _, deadline)| deadline)
    }

    pub fn completed(&mut self, native_window: usize) -> bool {
        if self
            .waiting
            .is_some_and(|(_, pointer, _)| pointer == native_window)
        {
            self.waiting = None;
            true
        } else {
            false
        }
    }
}

impl MacApplication {
    pub(super) fn advance_fullscreen_restore(&mut self) {
        if let Some((id, _, deadline)) = self.fullscreen_restore.waiting {
            let exists = self
                .windows
                .iter()
                .any(|app| app.window.as_ref().is_some_and(|window| window.id() == id));
            if exists && Instant::now() < deadline {
                return;
            }
            if exists {
                log::warn!("fullscreen restoration timed out for {id:?}");
            }
            self.fullscreen_restore.waiting = None;
        }
        while let Some(id) = self.fullscreen_restore.pending.pop_front() {
            let Some(window) = self
                .windows
                .iter()
                .find_map(|app| app.window.as_ref().filter(|window| window.id() == id))
            else {
                continue;
            };
            let Some(native) = native::native_window(window) else {
                continue;
            };
            self.fullscreen_restore.waiting = Some((
                id,
                objc2::rc::Retained::as_ptr(&native) as usize,
                Instant::now() + Duration::from_secs(10),
            ));
            // Set waiting before AppKit can synchronously emit notifications.
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            return;
        }
        if let Some(id) = self.fullscreen_restore.focus.take()
            && let Some(window) = self
                .windows
                .iter()
                .find_map(|app| app.window.as_ref().filter(|window| window.id() == id))
        {
            native::focus_window(window);
            self.active = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_or_duplicate_notifications_do_not_advance_restoration() {
        let id = WindowId::from(1);
        let mut queue = Queue::default();
        queue.pending.push_back(WindowId::from(2));
        queue.waiting = Some((id, 123, Instant::now()));
        assert!(!queue.completed(456));
        assert!(queue.waiting.is_some());
        assert!(queue.completed(123));
        assert!(!queue.completed(123));
        assert!(queue.active());
        assert_eq!(queue.pending.pop_front(), Some(WindowId::from(2)));
        assert!(!queue.active());
    }
}
