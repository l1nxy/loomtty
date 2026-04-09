use anyhow::Result;

/// Returns true if the autostart entry exists.
pub fn is_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        desktop_entry_path().is_some_and(|p| p.exists())
    }
    #[cfg(target_os = "macos")]
    {
        launchd_plist_path().is_some_and(|p| p.exists())
    }
    #[cfg(windows)]
    {
        is_enabled_windows()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

/// Write or remove the autostart entry.
pub fn set_enabled(enable: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        set_enabled_linux(enable)
    }
    #[cfg(target_os = "macos")]
    {
        set_enabled_macos(enable)
    }
    #[cfg(windows)]
    {
        set_enabled_windows(enable)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = enable;
        Ok(())
    }
}

// ─── Linux ──────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn desktop_entry_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("ciri-tray.desktop"))
}

#[cfg(target_os = "linux")]
fn set_enabled_linux(enable: bool) -> Result<()> {
    let path = desktop_entry_path().ok_or_else(|| anyhow::anyhow!("no config dir"))?;
    if enable {
        let exe = std::env::current_exe()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Ciri Tray\n\
             Comment=System tray for ciri terminal multiplexer\n\
             Exec={}\n\
             Hidden=false\n\
             NoDisplay=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&path, content)?;
        log::info!("wrote autostart entry: {}", path.display());
    } else if path.exists() {
        std::fs::remove_file(&path)?;
        log::info!("removed autostart entry: {}", path.display());
    }
    Ok(())
}

// ─── macOS ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn launchd_plist_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|d| d.join("Library/LaunchAgents/dev.ciri.tray.plist"))
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(target_os = "macos")]
fn set_enabled_macos(enable: bool) -> Result<()> {
    let path = launchd_plist_path().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    if enable {
        let exe = std::env::current_exe()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exe_escaped = xml_escape(&exe.to_string_lossy());
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.ciri.tray</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#,
            exe_escaped
        );
        std::fs::write(&path, content)?;
        log::info!("wrote launchd plist: {}", path.display());
    } else if path.exists() {
        std::fs::remove_file(&path)?;
        log::info!("removed launchd plist: {}", path.display());
    }
    Ok(())
}

// ─── Windows ────────────────────────────────────────────────────────

#[cfg(windows)]
fn is_enabled_windows() -> bool {
    use winreg::RegKey;
    use winreg::enums::*;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") else {
        return false;
    };
    run.get_value::<String, _>("CiriTray").is_ok()
}

#[cfg(windows)]
fn set_enabled_windows(enable: bool) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        KEY_SET_VALUE,
    )?;
    if enable {
        let exe = std::env::current_exe()?;
        run.set_value("CiriTray", &exe.to_string_lossy().to_string())?;
        log::info!("wrote registry autostart entry");
    } else {
        let _ = run.delete_value("CiriTray");
        log::info!("removed registry autostart entry");
    }
    Ok(())
}
