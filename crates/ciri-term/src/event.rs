use alacritty_terminal::event::{Event, EventListener};
use std::sync::mpsc;

/// Event listener that queues PtyWrite events for the pane to process.
#[derive(Clone)]
pub struct PtyEventListener {
    sender: mpsc::Sender<Event>,
}

impl PtyEventListener {
    pub fn new() -> (Self, mpsc::Receiver<Event>) {
        let (sender, receiver) = mpsc::channel();
        (PtyEventListener { sender }, receiver)
    }
}

impl EventListener for PtyEventListener {
    fn send_event(&self, event: Event) {
        if self.sender.send(event).is_err() {
            log::debug!("terminal event dropped: receiver closed");
        }
    }
}
