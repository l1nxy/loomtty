# Windows installer

[`loomtty.iss`](loomtty.iss) is an [Inno Setup](https://jrsoftware.org/isinfo.php)
script that builds `loomtty-<version>-x86_64-setup.exe`. The installer:

- lets the user pick **per-user** (no admin, `%LOCALAPPDATA%\Programs\loomtty`)
  or **all-users** (`Program Files`, elevated) at runtime;
- installs `loomtty.exe` (CLI), `loomtty-gui.exe` (GUI launcher),
  `loomtty-server.exe`, the shell-integration scripts (`loom.{bash,zsh,fish,ps1}`),
  the icon, and the licence;
- optionally adds the install directory to the user/system **PATH** and creates
  Start Menu / desktop shortcuts **pointing at `loomtty-gui.exe`**;
- registers an uninstaller (Add/Remove Programs) that stops the background
  daemon and reverses the PATH change.

User-data under `%APPDATA%\loom` (config) and `%LOCALAPPDATA%\loom` (sessions)
is intentionally left in place on uninstall.

## Why two GUI/CLI binaries

`loomtty.exe` is a console-subsystem binary so its CLI subcommands (`ls`, `web`,
`init`, `msg`, …) print synchronously and the shell waits for them. A
console-subsystem binary launched from a shortcut, though, makes Windows pop a
console window. So the GUI is shipped as a second binary, `loomtty-gui.exe`,
built for the GUI subsystem (`windows_subsystem = "windows"` in
`crates/loom/src/bin/loomtty-gui.rs`), and the shortcuts point at it — no
console window. Both binaries share all logic via the `loomtty` library crate.
This is the same split [WezTerm](https://wezterm.org/cli/general.html) uses
(`wezterm-gui.exe` vs `wezterm.exe`). The GUI binary is feature-gated behind
`gui-bin` so non-Windows builds don't link a redundant copy.

## Build locally

```powershell
# One-shot: build the release binaries and package them
dist\windows\build-installer.ps1 -Build

# Or, if target\release already has the binaries
dist\windows\build-installer.ps1
```

The script reads the version from `Cargo.toml`, auto-detects `ISCC.exe`, and
writes the result to `target\installer\`. Install Inno Setup if it's missing:

```powershell
winget install -e --id JRSoftware.InnoSetup
```

## Build manually

```powershell
# Build all three binaries first (the GUI launcher needs the gui-bin feature):
cargo build --release --features loomtty/gui-bin
ISCC.exe /DMyAppVersion=0.1.0 /DBinDir=..\..\target\release dist\windows\loomtty.iss
```

## CI

The [`Release`](../../.github/workflows/release.yml) workflow builds this
installer on every `v*` tag and attaches it to the GitHub Release alongside the
portable `.zip`.
