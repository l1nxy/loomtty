use super::App;

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
    pub fn play_bell_audio(&self) {
        let path = &self.config.terminal.bell_audio;
        if path.is_empty() {
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
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("afplay")
                    .arg(&path)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        });
    }
}
