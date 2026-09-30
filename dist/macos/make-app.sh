#!/bin/bash
# Build loomtty.app and loomtty-<version>-macos-arm64.dmg for macOS.
#
# Assembles the .app bundle (both binaries, Info.plist, icon), code-signs it
# (ad-hoc by default — pass --sign-id "Developer ID Application: …" for a real
# identity), and packages a drag-to-Applications .dmg.
#
# Usage (run from anywhere):
#   dist/macos/make-app.sh [--version X.Y.Z] [--bindir DIR] [--outdir DIR] [--sign-id ID] [--app-only] [--without-app-intents]
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
APP_ONLY=false
APP_INTENTS=true
while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --bindir)  BIN_DIR="$2"; shift 2 ;;
        --outdir)  OUT_DIR="$2"; shift 2 ;;
        --sign-id) SIGN_ID="$2"; shift 2 ;;
        --app-only) APP_ONLY=true; shift ;;
        --without-app-intents) APP_INTENTS=false; shift ;;
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

# Fail before touching an existing app. Full Xcode is needed for the system's
# discovery metadata; ordinary cargo builds still work with Command Line Tools.
if [ "$APP_INTENTS" = true ]; then
    xcrun --find appintentsmetadataprocessor >/dev/null 2>&1 || {
        echo "App Intents bundling requires full Xcode (xcrun cannot find appintentsmetadataprocessor)." >&2
        echo "Select Xcode with DEVELOPER_DIR, or use --without-app-intents for a development bundle." >&2
        exit 1
    }
fi
echo "loomtty.app  version=$VERSION  arch=arm64  sign=$SIGN_ID"
mkdir -p "$OUT_DIR"
BUILD_DIR="$(mktemp -d "$OUT_DIR/.loomtty-app.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT
APP="$BUILD_DIR/loomtty.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# The tray server is an AppKit process too. Give it its own bundle identity so
# Launch Services/AppleScript do not mistake it for the GUI application.
# Keep the sibling CLI entry point for existing clients and Homebrew links.
SERVER_APP="$APP/Contents/Helpers/loomtty-server.app"
mkdir -p "$SERVER_APP/Contents/MacOS"
install -m755 "$BIN_DIR/loomtty"        "$APP/Contents/MacOS/loomtty"
install -m755 "$BIN_DIR/loomtty-server" "$SERVER_APP/Contents/MacOS/loomtty-server"
sed "s/@VERSION@/$VERSION/g" "$SCRIPT_DIR/ServerInfo.plist" > "$SERVER_APP/Contents/Info.plist"
ln -s ../Helpers/loomtty-server.app/Contents/MacOS/loomtty-server "$APP/Contents/MacOS/loomtty-server"

# Info.plist (substitute version) + icon.
sed "s/@VERSION@/$VERSION/g" "$SCRIPT_DIR/Info.plist" > "$APP/Contents/Info.plist"
cp "$REPO_ROOT/assets/icons/app_icon_macos.icns" "$APP/Contents/Resources/app_icon_macos.icns"

cp "$SCRIPT_DIR/loomtty.sdef" "$APP/Contents/Resources/loomtty.sdef"

# Shell-integration scripts (OSC 133 prompt marks).
mkdir -p "$APP/Contents/Resources/shell-integration"
for s in loom.bash loom.zsh loom.fish; do
    cp "$REPO_ROOT/crates/loom-server/shell-integration/$s" "$APP/Contents/Resources/shell-integration/$s"
done

if [ "$APP_INTENTS" = true ]; then
    "$APP/Contents/MacOS/loomtty" _app-intents-metadata > "$BUILD_DIR/LoomAppIntents.swiftconstvalues"
    "$SCRIPT_DIR/app-intents/extract.sh" "$APP" "$BUILD_DIR/LoomAppIntents.swiftconstvalues"
fi

# Copy back-deployment Swift runtimes when the executable needs them. This is
# also needed when packaging a feature-enabled development binary without its
# discovery metadata. swift-stdlib-tool skips binaries without Swift references.
mkdir -p "$APP/Contents/Frameworks"
xcrun swift-stdlib-tool --copy --scan-executable "$APP/Contents/MacOS/loomtty" \
    --platform macosx --destination "$APP/Contents/Frameworks" \
    --sign "$SIGN_ID" --Xcodesign --timestamp=none
# The tool keeps pre-signing backups for incremental builds. They are not
# runtime dependencies and need not ship in this freshly assembled bundle.
rm -f "$APP/Contents/Frameworks/"*.original

# Sign inside-out: the extra helper binary first, then the bundle (which seals
# the main executable + the rest).
codesign --force --timestamp=none --sign "$SIGN_ID" "$SERVER_APP"
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP"
codesign --verify --deep --strict "$APP" && echo "codesign: verified"
# Preserve the last working bundle until all generation and signing succeeded.
rm -rf "$OUT_DIR/loomtty.app"
mv "$APP" "$OUT_DIR/loomtty.app"
APP="$OUT_DIR/loomtty.app"

if [ "$APP_ONLY" = true ]; then
    echo "Built: $APP"
    exit 0
fi

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
