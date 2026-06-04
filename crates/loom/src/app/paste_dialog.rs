use super::App;

impl App {
    /// Delegate: confirm pending paste.
    pub(crate) fn confirm_pending_paste(&mut self) {
        let Some(paste) = self.core.pending_paste.take() else {
            return;
        };

        match paste.target {
            super::PendingPasteTarget::Terminal => {
                self.send_paste_to_active_pane(paste.info.text.as_bytes());
            }
            super::PendingPasteTarget::CommandPalette => {
                if let Some(palette) = &mut self.core.command_palette {
                    palette.query.push_str(&paste.info.text);
                    self.filter_palette();
                }
            }
            super::PendingPasteTarget::Search => {
                if let Some(search) = &mut self.core.search_state {
                    search.query.push_str(&paste.info.text);
                    self.update_search_results();
                }
            }
        }
    }
}
