use super::Pane;

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

    pub(super) fn scan_osc_notifications(&mut self, data: &[u8]) {
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i] != 0x1b || data[i + 1] != b']' {
                i += 1;
                continue;
            }
            let osc_start = i + 2;
            let mut end = osc_start;
            while end < data.len() {
                if data[end] == 0x07 {
                    break;
                }
                if data[end] == 0x1b && end + 1 < data.len() && data[end + 1] == b'\\' {
                    break;
                }
                end += 1;
            }
            if end >= data.len() {
                break;
            }
            let payload = &data[osc_start..end];
            if payload.len() > Self::NOTIFICATION_PAYLOAD_MAX {
                i = if data[end] == 0x07 { end + 1 } else { end + 2 };
                continue;
            }
            if payload.starts_with(b"9;") {
                if self.rate_limit_accept() {
                    let message = String::from_utf8_lossy(&payload[2..]);
                    let message = Self::truncate_str(&message, Self::NOTIFICATION_BODY_MAX);
                    self.notifications_pending
                        .push(("Notification".to_string(), message));
                }
                i = if data[end] == 0x07 { end + 1 } else { end + 2 };
                continue;
            }
            if payload.starts_with(b"777;notify;") {
                if self.rate_limit_accept() {
                    let rest = &payload[b"777;notify;".len()..];
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
                i = if data[end] == 0x07 { end + 1 } else { end + 2 };
                continue;
            }
            i = if data[end] == 0x07 { end + 1 } else { end + 2 };
        }
    }
}
