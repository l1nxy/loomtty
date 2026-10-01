#!/bin/bash
# Compile-time extraction only: never launches the GUI or calls terminal actions.
set -euo pipefail
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$1"
CONST_VALUES="$2"
PROCESSOR="$(xcrun --find appintentsmetadataprocessor)"
COMPILER="$(xcrun --find swiftc)"
TOOLCHAIN="$(cd "$(dirname "$COMPILER")/../.." && pwd)"
SDK="$(xcrun --sdk macosx --show-sdk-path)"
XCODE_VERSION="$(xcodebuild -version | sed -n 's/^Build version //p')"
test -n "$XCODE_VERSION" || { echo "Cannot determine Xcode product build version" >&2; exit 1; }
LIST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/loom-intents-extract.XXXXXX")"
trap 'rm -rf "$LIST_DIR"' EXIT
printf '%s\n' "$SOURCE_DIR/Bridge.swift" "$SOURCE_DIR/Intents.swift" > "$LIST_DIR/sources"
printf '%s\n' "$CONST_VALUES" > "$LIST_DIR/const-values"
BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP/Contents/Info.plist")"
BINARY="$APP/Contents/MacOS/loomtty"
ARCH="$(lipo -archs "$BINARY")"
case "$ARCH" in
    arm64|x86_64) ;;
    *) echo "App Intents extraction requires a single-architecture binary, got: $ARCH" >&2; exit 1 ;;
esac
"$PROCESSOR" --toolchain-dir "$TOOLCHAIN" --sdk-root "$SDK" \
    --xcode-version "$XCODE_VERSION" \
    --module-name LoomAppIntents --bundle-identifier "$BUNDLE_ID" \
    --platform-family macOS --deployment-target 11.0 \
    --target-triple "$ARCH-apple-macos11.0" \
    --binary-file "$BINARY" --output "$APP/Contents/Resources" \
    --source-file-list "$LIST_DIR/sources" \
    --swift-const-vals-list "$LIST_DIR/const-values" --compile-time-extraction
# The extractor can exit successfully after skipping a target. Such a bundle
# would have Swift code but no discoverable Shortcuts actions: fail the build.
METADATA="$APP/Contents/Resources/Metadata.appintents"
for FILE in extract.actionsdata version.json; do
    test -s "$METADATA/$FILE" || { echo "App Intents metadata was not generated: $FILE" >&2; exit 1; }
done
