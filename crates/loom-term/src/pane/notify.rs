use super::Pane;
use crate::esc_scanner;

impl Pane {
    const NOTIFICATION_PAYLOAD_MAX: usize = 4096;
    const NOTIFICATION_TITLE_MAX: usize = 128;
    const NOTIFICATION_BODY_MAX: usize = 256;
    const NOTIFICATION_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

    fn truncate_str(s: &str, max_bytes: usize) -> String {
        if s.len() <= max_bytes {
            return s.to_string();
        }
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }

    pub(super) fn rate_limit_accept(&mut self) -> bool {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_notification_time
            && now.duration_since(last) < Self::NOTIFICATION_MIN_INTERVAL
        {
            return false;
        }
        self.last_notification_time = Some(now);
        true
    }

    /// Parse a single OSC 9 payload and push a notification if it's a toast.
    ///
    /// OSC 9 has two conflicting dialects:
    ///   * iTerm2:  `ESC ] 9 ; <text> ST`               — plain notification
    ///   * ConEmu:  `ESC ] 9 ; <digit> ; <args...> ST`  — subcommand
    ///
    /// ConEmu subcommand 2 is a toast message box; the others (1=title,
    /// 4=progress, 9=cwd, 11=bell, ...) are not notifications.  We disambiguate
    /// by looking at whether the payload starts with `<digits>;`.
    fn handle_osc_9(&mut self, payload: &[u8]) {
        if payload.len() > Self::NOTIFICATION_PAYLOAD_MAX {
            return;
        }
        let digits = payload.iter().take_while(|b| b.is_ascii_digit()).count();
        let is_conemu_subcommand = digits > 0 && payload.get(digits) == Some(&b';');

        let message_bytes = if is_conemu_subcommand {
            if !payload.starts_with(b"2;") {
                return; // not a toast subcommand — ignore
            }
            &payload[2..]
        } else {
            payload
        };

        if !self.rate_limit_accept() {
            return;
        }
        let message = String::from_utf8_lossy(message_bytes);
        let message = Self::truncate_str(&message, Self::NOTIFICATION_BODY_MAX);
        self.notifications_pending
            .push(("Notification".to_string(), message));
    }

    /// Parse a single OSC 777 payload (xterm notification extension).
    ///
    /// Format: `ESC ] 777 ; notify ; <title> [ ; <body> ] ST`
    fn handle_osc_777(&mut self, payload: &[u8]) {
        if payload.len() > Self::NOTIFICATION_PAYLOAD_MAX {
            return;
        }
        let Some(rest) = payload.strip_prefix(b"notify;") else {
            return;
        };
        if !self.rate_limit_accept() {
            return;
        }
        let rest_str = String::from_utf8_lossy(rest);
        let (title, body) = match rest_str.split_once(';') {
            Some((t, b)) => (
                Self::truncate_str(t, Self::NOTIFICATION_TITLE_MAX),
                Self::truncate_str(b, Self::NOTIFICATION_BODY_MAX),
            ),
            None => (
                Self::truncate_str(&rest_str, Self::NOTIFICATION_TITLE_MAX),
                String::new(),
            ),
        };
        self.notifications_pending.push((title, body));
    }

    pub(super) fn scan_osc_notifications(&mut self, data: &[u8]) {
        for (_, payload) in esc_scanner::scan_osc(data, b"9").sequences {
            self.handle_osc_9(payload);
        }
        for (_, payload) in esc_scanner::scan_osc(data, b"777").sequences {
            self.handle_osc_777(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pane::Pane;

    fn shell_path() -> &'static str {
        if std::path::Path::new("/bin/sh").exists() {
            "/bin/sh"
        } else {
            "sh"
        }
    }

    fn test_pane() -> Pane {
        Pane::new(1, 4, 3, shell_path()).expect("pane")
    }

    #[test]
    fn osc9_itermstyle_plain_notification() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;hello world\x07");
        let notes = pane.drain_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "Notification");
        assert_eq!(notes[0].1, "hello world");
    }

    #[test]
    fn osc9_conemu_progress_is_ignored() {
        // OSC 9;4;0 — ConEmu/Windows-Terminal progress bar clear.
        // Must NOT produce a notification.
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;4;0\x07");
        assert!(pane.drain_notifications().is_empty());
    }

    #[test]
    fn osc9_conemu_progress_with_value_is_ignored() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;4;1;75\x07");
        assert!(pane.drain_notifications().is_empty());
    }

    #[test]
    fn osc9_conemu_title_subcommand_is_ignored() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;1;my title\x07");
        assert!(pane.drain_notifications().is_empty());
    }

    #[test]
    fn osc9_conemu_toast_subcommand_is_notification() {
        // OSC 9;2;<message> — ConEmu message box → treat as toast.
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;2;build finished\x07");
        let notes = pane.drain_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1, "build finished");
    }

    #[test]
    fn osc9_esc_backslash_terminator() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;msg\x1b\\");
        let notes = pane.drain_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1, "msg");
    }

    #[test]
    fn osc777_notify_with_title_and_body() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]777;notify;Build;done\x07");
        let notes = pane.drain_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "Build");
        assert_eq!(notes[0].1, "done");
    }

    #[test]
    fn osc777_notify_title_only() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]777;notify;Ping\x07");
        let notes = pane.drain_notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "Ping");
        assert_eq!(notes[0].1, "");
    }

    #[test]
    fn osc777_non_notify_subcommand_ignored() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]777;other;data\x07");
        assert!(pane.drain_notifications().is_empty());
    }

    #[test]
    fn rate_limit_drops_rapid_duplicates() {
        let mut pane = test_pane();
        pane.scan_osc_notifications(b"\x1b]9;first\x07\x1b]9;second\x07");
        let notes = pane.drain_notifications();
        // Second should be dropped by the 1s rate-limit window.
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1, "first");
    }
}
