//! `loomtty-gui` — the GUI entry point.
//!
//! On Windows this binary is built for the GUI subsystem (`windows_subsystem =
//! "windows"`) so launching it from a Start Menu / desktop shortcut opens the
//! window without ever allocating a console. It shares all logic with the
//! `loomtty` CLI binary via the library crate; the only difference is the
//! subsystem. This is the same split WezTerm uses (`wezterm-gui.exe` vs
//! `wezterm.exe`).
//!
//! Because there is no console, anything written to stderr (panics, fatal
//! startup errors like a GPU/driver init failure or a bad config) would
//! otherwise vanish — the icon would just "do nothing". So on Windows we route
//! both a panic hook and the top-level error to a message box.
//!
//! On non-Windows targets the attribute is a no-op and this is just a second,
//! identical launcher (only built when the `gui-bin` feature is enabled).
#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::Result;
use clap::Parser;
use loomtty::cli::{self, Cli};

fn main() -> Result<()> {
    #[cfg(windows)]
    install_panic_dialog();

    loomtty::init_logging();
    let cli = cli::resolve(Cli::parse());
    let result = loomtty::run(cli);

    #[cfg(windows)]
    if let Err(ref e) = result {
        show_error_dialog(&format!("loomtty failed to start:\n\n{e:#}"));
    }
    result
}

/// Install a panic hook that pops a message box, so a panic in the GUI binary
/// isn't swallowed by the missing console. The default hook still runs first
/// (harmless — its stderr output just goes nowhere here).
#[cfg(windows)]
fn install_panic_dialog() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        show_error_dialog(&format!("loomtty crashed:\n\n{info}"));
    }));
}

/// Show a modal error dialog (user32 `MessageBoxW`). Declared inline to avoid
/// pulling in a Windows-bindings crate, matching the style used elsewhere in
/// the workspace (see `connection.rs`).
#[cfg(windows)]
fn show_error_dialog(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, u_type: u32) -> i32;
    }
    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let text = wide(message);
    let caption = wide("loomtty");
    unsafe {
        MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
    }
}
