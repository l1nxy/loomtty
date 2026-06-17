# Installing loomtty — AI Agent Guide

For AI agents: read this, then install loomtty for the user. All steps are unprivileged.

## 1. Download

Detect platform and download the right binary from GitHub Releases:

```sh
# Detect
OS=$(uname -s | tr '[:upper:]' '[:lower:]')    # linux / darwin
ARCH=$(uname -m)                                 # x86_64 / aarch64 / arm64
[ "$ARCH" = "arm64" ] && ARCH="aarch64"

case "$OS" in
  linux)  ARTIFACT="loomtty-linux-${ARCH}" ; EXT="tar.gz" ;;
  darwin) ARTIFACT="loomtty-macos-${ARCH}" ; EXT="tar.gz" ;;
  *)      echo "Download manually from https://github.com/l1nxy/loomtty/releases" ; exit 1 ;;
esac

# Get latest release URL and download
curl -sL "https://github.com/l1nxy/loomtty/releases/latest/download/${ARTIFACT}.${EXT}" -o "/tmp/${ARTIFACT}.${EXT}"
```

For Windows: prefer the installer — see the [Windows](#windows) section below.

## 2. Install

```sh
# Extract
mkdir -p ~/.local/bin
tar xzf "/tmp/${ARTIFACT}.${EXT}" -C /tmp/
cp "/tmp/${ARTIFACT}/loomtty" "/tmp/${ARTIFACT}/loomtty-server" ~/.local/bin/
chmod +x ~/.local/bin/loomtty ~/.local/bin/loomtty-server
```

Ensure `~/.local/bin` is in PATH:
```sh
case "$SHELL" in
  */fish) fish -c 'fish_add_path ~/.local/bin' 2>/dev/null ;;
  */zsh)  grep -q 'local/bin' ~/.zshrc 2>/dev/null || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc ;;
  *)      grep -q 'local/bin' ~/.bashrc 2>/dev/null || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc ;;
esac
```

Verify: `~/.local/bin/loomtty --version`

## 3. Shell Integration (recommended)

The release tarball includes shell integration scripts:

```sh
case "$SHELL" in
  */fish)
    mkdir -p ~/.config/fish/conf.d
    cp "/tmp/${ARTIFACT}/shell-integration/loom.fish" ~/.config/fish/conf.d/
    ;;
  */zsh)
    mkdir -p ~/.config/loom/shell-integration
    cp "/tmp/${ARTIFACT}/shell-integration/loom.zsh" ~/.config/loom/shell-integration/
    grep -q 'loom.zsh' ~/.zshrc 2>/dev/null || echo 'source ~/.config/loom/shell-integration/loom.zsh' >> ~/.zshrc
    ;;
esac
```

## 4. Desktop Entry (Linux, optional)

```sh
if [ "$OS" = "linux" ]; then
  mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
  cp "/tmp/${ARTIFACT}/dist/linux/com.github.l1nxy.loom.desktop" ~/.local/share/applications/
  cp "/tmp/${ARTIFACT}/icons/icon.svg" ~/.local/share/icons/hicolor/scalable/apps/com.github.l1nxy.loom.svg
fi
```

## Windows

The Unix steps above (`tar`, `~/.local/bin`, desktop entries) don't apply on
Windows. Use one of these instead.

### Installer (recommended)

Download `loomtty-<version>-x86_64-setup.exe` from
[Releases](https://github.com/l1nxy/loomtty/releases) and run it. The setup
wizard lets the user choose:

- **Install for me only** — no admin prompt, installs to
  `%LOCALAPPDATA%\Programs\loomtty`.
- **Install for all users** — elevates, installs to `Program Files`.

It adds loomtty to PATH (recommended task, on by default), creates a Start Menu
shortcut, and registers an uninstaller in *Apps & features*. Both `loomtty.exe`
and `loomtty-server.exe` land in the same directory, which is how the client
finds the daemon. After installing, open a **new** terminal and verify:

```powershell
loomtty --version
```

Silent install (e.g. for scripted/agent setup):

```powershell
# per-user, no UAC
.\loomtty-<version>-x86_64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /CURRENTUSER
# all-users (must already be elevated)
.\loomtty-<version>-x86_64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /ALLUSERS
```

### Portable zip

Download `loomtty-windows-x86_64.zip`, extract it to a directory of your choice
(keeping `loomtty.exe` and `loomtty-server.exe` together), and add that
directory to your PATH manually.

### Shell integration (PowerShell)

The installer/zip ships `shell-integration\loom.ps1` (OSC 133 prompt marks +
working-directory reports). Enable it by dot-sourcing it from your profile:

```powershell
notepad $PROFILE   # create/edit your PowerShell profile, then add:
. "$env:LOCALAPPDATA\Programs\loomtty\shell-integration\loom.ps1"
```

(Use the install directory you chose; for an all-users install it's under
`%ProgramFiles%\loomtty`.)

## 5. Initial Setup

`loomtty init` is an interactive TUI wizard — the AI cannot run it non-interactively.
**Ask the user** to run it themselves:

> Agent instruction: use your "ask user" capability (e.g. AskUserQuestion tool
> in Claude Code, or suggest `! loomtty init` so it runs in the current session)
> to have the user execute:
>
> ```
> loomtty init
> ```
>
> It will guide them through:
> - Keybinding style: prefix (tmux-like) or sticky (zellij-like)
> - Leader key: Ctrl+W, Ctrl+A, Alt, or custom
> - Color theme: loom_dark, one_dark, one_half_dark, catppuccin_mocha, tokyo_night, dracula, nord, gruvbox_dark, ghostty
> - Status bar position: top or bottom
>
> Shell and font are auto-detected. If the user wants to skip, loomtty works
> with all defaults (leader=Ctrl+W, prefix mode).

## Done

```sh
loomtty   # Launch (auto-starts server)
```

## Building from Source

Only needed for development. Requires Rust 1.82+ and platform libs:

```sh
git clone https://github.com/l1nxy/loomtty.git && cd loomtty
cargo build --release
cp target/release/loomtty target/release/loomtty-server ~/.local/bin/
```

If the build fails due to missing system libraries, the error message will name the missing package (e.g. `freetype2`, `fontconfig`, `xcb`). Install the corresponding `-dev` / `-devel` package for your distro and retry.

## Uninstall

### Unix

```sh
rm -f ~/.local/bin/loomtty ~/.local/bin/loomtty-server
rm -rf ~/.config/loom
rm -f ~/.config/fish/conf.d/loom.fish
rm -f ~/.local/share/applications/com.github.l1nxy.loom.desktop
rm -f ~/.local/share/icons/hicolor/*/apps/com.github.l1nxy.loom.*
```

### Windows

Uninstall from *Settings → Apps → Installed apps → loomtty* (or run the
uninstaller in the install directory). It stops the daemon, removes the files,
and reverses the PATH change. User data is left behind — delete it manually if
desired:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\loom"        # config
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\loom"   # sessions / state
```

For a portable-zip install, just delete the extracted folder and remove it from
PATH.
