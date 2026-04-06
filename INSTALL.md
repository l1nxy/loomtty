# Installing ciri — AI Agent Guide

For AI agents: read this, then install ciri for the user. All steps are unprivileged.

## 1. Download

Detect platform and download the right binary from GitHub Releases:

```sh
# Detect
OS=$(uname -s | tr '[:upper:]' '[:lower:]')    # linux / darwin
ARCH=$(uname -m)                                 # x86_64 / aarch64 / arm64
[ "$ARCH" = "arm64" ] && ARCH="aarch64"

case "$OS" in
  linux)  ARTIFACT="ciri-linux-${ARCH}" ; EXT="tar.gz" ;;
  darwin) ARTIFACT="ciri-macos-${ARCH}" ; EXT="tar.gz" ;;
  *)      echo "Download manually from https://github.com/l1nxy/ciri/releases" ; exit 1 ;;
esac

# Get latest release URL and download
curl -sL "https://github.com/l1nxy/ciri/releases/latest/download/${ARTIFACT}.${EXT}" -o "/tmp/${ARTIFACT}.${EXT}"
```

For Windows: ask the user to download `ciri-windows-x86_64.zip` from
https://github.com/l1nxy/ciri/releases and extract it to a directory in their PATH.

## 2. Install

```sh
# Extract
mkdir -p ~/.local/bin
tar xzf "/tmp/${ARTIFACT}.${EXT}" -C /tmp/
cp "/tmp/${ARTIFACT}/ciri" "/tmp/${ARTIFACT}/ciri-server" ~/.local/bin/
chmod +x ~/.local/bin/ciri ~/.local/bin/ciri-server
```

Ensure `~/.local/bin` is in PATH:
```sh
case "$SHELL" in
  */fish) fish -c 'fish_add_path ~/.local/bin' 2>/dev/null ;;
  */zsh)  grep -q 'local/bin' ~/.zshrc 2>/dev/null || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc ;;
  *)      grep -q 'local/bin' ~/.bashrc 2>/dev/null || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc ;;
esac
```

Verify: `~/.local/bin/ciri --version`

## 3. Shell Integration (recommended)

The release tarball includes shell integration scripts:

```sh
case "$SHELL" in
  */fish)
    mkdir -p ~/.config/fish/conf.d
    cp "/tmp/${ARTIFACT}/shell-integration/ciri.fish" ~/.config/fish/conf.d/
    ;;
  */zsh)
    mkdir -p ~/.config/ciri/shell-integration
    cp "/tmp/${ARTIFACT}/shell-integration/ciri.zsh" ~/.config/ciri/shell-integration/
    grep -q 'ciri.zsh' ~/.zshrc 2>/dev/null || echo 'source ~/.config/ciri/shell-integration/ciri.zsh' >> ~/.zshrc
    ;;
esac
```

## 4. Desktop Entry (Linux, optional)

```sh
if [ "$OS" = "linux" ]; then
  mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
  cp "/tmp/${ARTIFACT}/dist/linux/com.github.l1nxy.ciri.desktop" ~/.local/share/applications/
  cp "/tmp/${ARTIFACT}/icons/icon.svg" ~/.local/share/icons/hicolor/scalable/apps/com.github.l1nxy.ciri.svg
fi
```

## 5. Initial Setup

`ciri init` is an interactive TUI wizard — the AI cannot run it non-interactively.
**Ask the user** to run it themselves:

> Agent instruction: use your "ask user" capability (e.g. AskUserQuestion tool
> in Claude Code, or suggest `! ciri init` so it runs in the current session)
> to have the user execute:
>
> ```
> ciri init
> ```
>
> It will guide them through:
> - Keybinding style: prefix (tmux-like) or sticky (zellij-like)
> - Leader key: Ctrl+W, Ctrl+A, Alt, or custom
> - Color theme: one_dark, catppuccin_mocha, tokyo_night, dracula, nord, gruvbox_dark
> - Status bar position: top or bottom
>
> Shell and font are auto-detected. If the user wants to skip, ciri works
> with all defaults (leader=Ctrl+W, prefix mode).

## Done

```sh
ciri   # Launch (auto-starts server)
```

## Building from Source

Only needed for development. Requires Rust 1.82+ and platform libs:

```sh
git clone https://github.com/l1nxy/ciri.git && cd ciri
cargo build --release
cp target/release/ciri target/release/ciri-server ~/.local/bin/
```

If the build fails due to missing system libraries, the error message will name the missing package (e.g. `freetype2`, `fontconfig`, `xcb`). Install the corresponding `-dev` / `-devel` package for your distro and retry.

## Uninstall

```sh
rm -f ~/.local/bin/ciri ~/.local/bin/ciri-server
rm -rf ~/.config/ciri
rm -f ~/.config/fish/conf.d/ciri.fish
rm -f ~/.local/share/applications/com.github.l1nxy.ciri.desktop
rm -f ~/.local/share/icons/hicolor/*/apps/com.github.l1nxy.ciri.*
```
