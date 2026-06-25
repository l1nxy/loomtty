#!/bin/sh
# loomtty installer for Linux.
#
# Downloads the latest release and installs it for the current user (no root):
#   curl -fsSL https://raw.githubusercontent.com/l1nxy/loomtty/dev/install.sh | sh
#
# Options (pass after `-- ` when piping to sh, e.g. `… | sh -s -- --system`):
#   --user            install under ~/.local  (default, no root)
#   --system          install under /usr/local (uses sudo if not root)
#   --version=vX.Y.Z  install a specific release tag (default: latest)
#   --uninstall       remove a previous install (respects --user/--system)
#   --help
set -eu

REPO="l1nxy/loomtty"
APP_ID="com.github.l1nxy.loom"

SCOPE="user"
ACTION="install"
VERSION="latest"
for arg in "$@"; do
    case "$arg" in
        --system) SCOPE="system" ;;
        --user) SCOPE="user" ;;
        --uninstall) ACTION="uninstall" ;;
        --version=*) VERSION="${arg#--version=}" ;;
        -h|--help) ACTION="help" ;;
        *) printf 'error: unknown option: %s\n' "$arg" >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

if [ "$ACTION" = "help" ]; then
    # A literal heredoc, not `sed "$0"`: under `curl … | sh -s -- --help` $0 is
    # "sh", not this script, so reading help from $0 would print nothing.
    cat <<'EOF'
loomtty installer for Linux.

Downloads the latest release and installs it for the current user (no root):
  curl -fsSL https://raw.githubusercontent.com/l1nxy/loomtty/dev/install.sh | sh

Options (pass after `-- ` when piping to sh, e.g. `… | sh -s -- --system`):
  --user            install under ~/.local  (default, no root)
  --system          install under /usr/local (uses sudo if not root)
  --version=vX.Y.Z  install a specific release tag (default: latest)
  --uninstall       remove a previous install (respects --user/--system)
  --help
EOF
    exit 0
fi

# ── Resolve install prefix ──────────────────────────────────────────────
SUDO=""
if [ "$SCOPE" = "system" ]; then
    PREFIX="/usr/local"
    if [ "$(id -u)" -ne 0 ]; then
        if have sudo; then SUDO="sudo"; else err "--system needs root; re-run as root or install sudo"; fi
    fi
else
    PREFIX="$HOME/.local"
fi
BIN_DIR="$PREFIX/bin"
DATA_DIR="$PREFIX/share"
run() { if [ -n "$SUDO" ]; then $SUDO "$@"; else "$@"; fi; }

refresh_caches() {
    if have update-desktop-database; then run update-desktop-database "$DATA_DIR/applications" >/dev/null 2>&1 || true; fi
    if have gtk-update-icon-cache; then run gtk-update-icon-cache -qtf "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true; fi
}

# ── Uninstall ───────────────────────────────────────────────────────────
if [ "$ACTION" = "uninstall" ]; then
    say "Removing loomtty from $PREFIX…"
    run rm -f "$BIN_DIR/loomtty" "$BIN_DIR/loomtty-server"
    run rm -f "$DATA_DIR/applications/$APP_ID.desktop"
    run rm -f "$DATA_DIR/metainfo/$APP_ID.metainfo.xml"
    run rm -f "$DATA_DIR"/icons/hicolor/*/apps/"$APP_ID".png "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
    run rm -rf "$DATA_DIR/loomtty"
    refresh_caches
    say "Done. Your config (~/.config/loom) and sessions were left in place."
    exit 0
fi

# ── Detect platform ─────────────────────────────────────────────────────
[ "$(uname -s)" = "Linux" ] || err "this installer is for Linux; see INSTALL.md for macOS/Windows"
case "$(uname -m)" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) err "unsupported architecture: $(uname -m)" ;;
esac
ARTIFACT="loomtty-linux-$ARCH"

if [ "$VERSION" = "latest" ]; then
    URL="https://github.com/$REPO/releases/latest/download/$ARTIFACT.tar.gz"
else
    URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT.tar.gz"
fi

# ── Download + extract ──────────────────────────────────────────────────
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM
say "Downloading $ARTIFACT ($VERSION)…"
if have curl; then
    curl -fSL "$URL" -o "$TMP/pkg.tar.gz" || err "download failed: $URL"
elif have wget; then
    wget -O "$TMP/pkg.tar.gz" "$URL" || err "download failed: $URL"
else
    err "need curl or wget to download"
fi
tar -xzf "$TMP/pkg.tar.gz" -C "$TMP" || err "failed to extract archive"
SRC="$TMP/$ARTIFACT"
[ -f "$SRC/loomtty" ] || err "unexpected archive layout (no loomtty in $ARTIFACT/)"

# ── Install binaries ────────────────────────────────────────────────────
say "Installing to $BIN_DIR…"
run mkdir -p "$BIN_DIR"
run install -m755 "$SRC/loomtty" "$BIN_DIR/loomtty"
run install -m755 "$SRC/loomtty-server" "$BIN_DIR/loomtty-server"

# ── Desktop entry + AppStream metadata ──────────────────────────────────
if [ -f "$SRC/dist/linux/$APP_ID.desktop" ]; then
    run mkdir -p "$DATA_DIR/applications"
    # Rewrite Exec/TryExec to the absolute binary path. Desktop launchers run
    # from the session environment, which often lacks ~/.local/bin (we only add
    # it to the shell rc), so a bare `Exec=loomtty` would leave the menu entry
    # unable to launch even though the install succeeded.
    sed -E "s#^(Exec|TryExec)=loomtty\b#\1=$BIN_DIR/loomtty#" \
        "$SRC/dist/linux/$APP_ID.desktop" > "$TMP/$APP_ID.desktop"
    run install -m644 "$TMP/$APP_ID.desktop" "$DATA_DIR/applications/$APP_ID.desktop"
fi
if [ -f "$SRC/dist/linux/$APP_ID.metainfo.xml" ]; then
    run mkdir -p "$DATA_DIR/metainfo"
    run install -m644 "$SRC/dist/linux/$APP_ID.metainfo.xml" "$DATA_DIR/metainfo/$APP_ID.metainfo.xml"
fi

# ── Icons (hicolor theme) ───────────────────────────────────────────────
for png in "$SRC"/icons/icon-*x*.png; do
    [ -f "$png" ] || continue
    size="$(basename "$png" | sed -n 's/^icon-\([0-9]\{1,\}\)x[0-9]\{1,\}\.png$/\1/p')"
    [ -n "$size" ] || continue
    run mkdir -p "$DATA_DIR/icons/hicolor/${size}x${size}/apps"
    run install -m644 "$png" "$DATA_DIR/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
if [ -f "$SRC/icons/icon.svg" ]; then
    run mkdir -p "$DATA_DIR/icons/hicolor/scalable/apps"
    run install -m644 "$SRC/icons/icon.svg" "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
fi

# ── Shell integration ───────────────────────────────────────────────────
run mkdir -p "$DATA_DIR/loomtty/shell-integration"
for f in loom.bash loom.zsh loom.fish; do
    [ -f "$SRC/shell-integration/$f" ] && run install -m644 "$SRC/shell-integration/$f" "$DATA_DIR/loomtty/shell-integration/$f"
done

refresh_caches

# ── PATH hint (user scope) ──────────────────────────────────────────────
on_path=0
case ":$PATH:" in *":$BIN_DIR:"*) on_path=1 ;; esac
if [ "$SCOPE" = "user" ] && [ "$on_path" -eq 0 ]; then
    case "${SHELL##*/}" in
        fish)
            mkdir -p "$HOME/.config/fish"
            have fish && fish -c 'fish_add_path -U $HOME/.local/bin' 2>/dev/null || true ;;
        zsh)
            rc="$HOME/.zshrc"
            grep -qs '.local/bin' "$rc" 2>/dev/null || printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc" ;;
        *)
            rc="$HOME/.profile"; [ -f "$HOME/.bashrc" ] && rc="$HOME/.bashrc"
            grep -qs '.local/bin' "$rc" 2>/dev/null || printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc" ;;
    esac
    say ""
    say "Added $BIN_DIR to your PATH — open a new terminal (or 'source' your shell rc) to pick it up."
fi

say ""
say "loomtty installed. Verify with:  $BIN_DIR/loomtty --version"
say "Shell integration (optional, OSC 133 prompt marks):"
say "  echo 'source $DATA_DIR/loomtty/shell-integration/loom.bash' >> ~/.bashrc   # or loom.zsh / loom.fish"
say "First-time setup:  loomtty init"
