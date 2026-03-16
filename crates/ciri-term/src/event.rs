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
        let _ = self.sender.send(event);
    }
}
