use super::App;

impl App {
    /// Delegate: confirm pending paste.
    pub(crate) fn confirm_pending_paste(&mut self) {
        self.core.confirm_pending_paste();
    }
}
