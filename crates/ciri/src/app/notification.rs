use std::sync::atomic::{AtomicBool, Ordering};

use super::App;

static AUDIO_PLAYING: AtomicBool = AtomicBool::new(false);

impl App {
    /// Send a desktop notification (Linux/macOS only).
    pub fn send_desktop_notification(&self, summary: &str, body: &str) {
        #[cfg(unix)]
        {
            use notify_rust::Notification;
            let _ = Notification::new()
                .summary(summary)
                .body(body)
                .appname("ciri")
                .timeout(5000)
                .show();
        }
        #[cfg(not(unix))]
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
            AUDIO_PLAYING.store(false, Ordering::SeqCst);
        });
    }
}
