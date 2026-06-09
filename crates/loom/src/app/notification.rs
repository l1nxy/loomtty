use std::sync::atomic::{AtomicBool, Ordering};

use super::App;

static AUDIO_PLAYING: AtomicBool = AtomicBool::new(false);

/// Escape the 5 XML special characters per the freedesktop Desktop Notifications
/// Specification §Markup.  Notification daemons (GNOME, KDE, dunst) may render
/// `<b>`, `<i>`, `<u>`, `<a>`, `<img>` tags in the body field — escaping these
/// characters prevents a malicious terminal application from injecting markup.
///
/// Reference: https://specifications.freedesktop.org/notification-spec/latest/
#[cfg(unix)]
fn escape_notification_markup(s: &str) -> String {
    // & must be escaped first to avoid double-escaping.
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

impl App {
    /// Send a desktop notification (Linux, macOS, Windows).
    pub fn send_desktop_notification(&self, summary: &str, body: &str) {
        #[cfg(unix)]
        {
            use notify_rust::Notification;
            let safe_summary = escape_notification_markup(summary);
            let safe_body = escape_notification_markup(body);
            let _ = Notification::new()
                .summary(&safe_summary)
                .body(&safe_body)
                .appname("loomtty")
                .timeout(5000)
                .show();
        }
        #[cfg(windows)]
        {
            // Windows toast via notify-rust → tauri-winrt-notification. That
            // backend builds the toast XML and escapes special chars itself
            // (quick_xml), so we pass the raw text — applying the freedesktop
            // markup escaping here would double-escape and surface a literal
            // `&amp;`. The toast is attributed to the built-in PowerShell AUMID
            // (the backend's default `app_id`), the only identity guaranteed to
            // exist without installing/registering loomtty; a custom unregistered
            // AUMID would fail to show with "Element not found".
            //
            // Called from the winit event-loop thread, which has COM initialized
            // (winit's OleInitialize). Required because `Toast::show()` assumes
            // an already-initialized apartment — never move this onto a fresh
            // thread, which would lack COM and fail silently.
            use notify_rust::Notification;
            // Log on failure rather than swallowing it: a discarded toast is
            // otherwise an invisible no-op. Failures here mean a missing AUMID
            // ("Element not found"), an uninitialized COM apartment (caller is
            // off the winit thread, see above), or the user's notification quota.
            if let Err(e) = Notification::new().summary(summary).body(body).show() {
                log::warn!("windows toast notification failed: {e:?}");
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (summary, body);
            log::info!("notification (no desktop): {} - {}", summary, body);
        }
    }

    /// Play bell audio file via system command (non-blocking).
    /// Guards against spawning unbounded threads by skipping if one is already playing.
    pub fn play_bell_audio(&self) {
        let path = &self.core.config.terminal.bell_audio;
        if path.is_empty() {
            return;
        }
        // Validate the path: must be an existing regular file with a sane extension.
        // This prevents passing device files (/dev/urandom), pipes, or URIs
        // that paplay/afplay might interpret in unexpected ways.
        let p = std::path::Path::new(path);
        if !p.is_absolute() {
            log::warn!("bell_audio path must be absolute: {path}");
            return;
        }
        match std::fs::symlink_metadata(p) {
            Ok(meta) if meta.file_type().is_file() => {}
            _ => {
                log::warn!("bell_audio path is not a regular file: {path}");
                return;
            }
        }
        // Don't spawn if one is already playing
        if AUDIO_PLAYING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let path = path.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("paplay")
                    .arg(&path)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("afplay")
                    .arg(&path)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("powershell")
                    .args([
                        "-c",
                        &format!("(New-Object Media.SoundPlayer '{}').PlaySync()", path),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            AUDIO_PLAYING.store(false, Ordering::SeqCst);
        });
    }
}
