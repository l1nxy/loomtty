#!/bin/bash
# Build loomtty.app and loomtty-<version>-macos-arm64.dmg for macOS.
#
# Assembles the .app bundle (both binaries, Info.plist, icon), code-signs it
# (ad-hoc by default — pass --sign-id "Developer ID Application: …" for a real
# identity), and packages a drag-to-Applications .dmg.
#
# Usage (run from anywhere):
#   dist/macos/make-app.sh [--version X.Y.Z] [--bindir DIR] [--outdir DIR] [--sign-id ID]
#
# Defaults: version from Cargo.toml, bindir=target/release, outdir=target/macos,
# sign-id="-" (ad-hoc).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

VERSION=""
BIN_DIR="$REPO_ROOT/target/release"
OUT_DIR="$REPO_ROOT/target/macos"
SIGN_ID="-"
while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --bindir)  BIN_DIR="$2"; shift 2 ;;
        --outdir)  OUT_DIR="$2"; shift 2 ;;
        --sign-id) SIGN_ID="$2"; shift 2 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$VERSION" ]; then
    VERSION="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
fi
[ -n "$VERSION" ] || { echo "could not determine version" >&2; exit 1; }

for b in loomtty loomtty-server; do
    [ -f "$BIN_DIR/$b" ] || { echo "missing binary: $BIN_DIR/$b (build with: cargo build --release -p loomtty -p loomtty-server)" >&2; exit 1; }
done

echo "loomtty.app  version=$VERSION  arch=arm64  sign=$SIGN_ID"
mkdir -p "$OUT_DIR"
APP="$OUT_DIR/loomtty.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Binaries (the daemon sits next to the client, where the client looks for it).
install -m755 "$BIN_DIR/loomtty"        "$APP/Contents/MacOS/loomtty"
install -m755 "$BIN_DIR/loomtty-server" "$APP/Contents/MacOS/loomtty-server"

# Info.plist (substitute version) + icon.
sed "s/@VERSION@/$VERSION/g" "$SCRIPT_DIR/Info.plist" > "$APP/Contents/Info.plist"
cp "$REPO_ROOT/assets/icons/app_icon_macos.icns" "$APP/Contents/Resources/app_icon_macos.icns"

# Shell-integration scripts (OSC 133 prompt marks).
mkdir -p "$APP/Contents/Resources/shell-integration"
for s in loom.bash loom.zsh loom.fish; do
    cp "$REPO_ROOT/crates/loom-server/shell-integration/$s" "$APP/Contents/Resources/shell-integration/$s"
done

# Sign inside-out: the extra helper binary first, then the bundle (which seals
# the main executable + the rest).
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP/Contents/MacOS/loomtty-server"
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP"
codesign --verify --deep --strict "$APP" && echo "codesign: verified"

# ── .dmg (drag-to-Applications) ─────────────────────────────────────────
DMG="$OUT_DIR/loomtty-$VERSION-macos-arm64.dmg"
STAGE="$OUT_DIR/dmg-stage"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "loomtty" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

echo ""
echo "Built:"
echo "  $APP"
echo "  $DMG"
