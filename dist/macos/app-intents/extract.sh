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
BINARY="$APP/Contents/MacOS/loomtty"
ARCH="$(lipo -archs "$BINARY")"
case "$ARCH" in
    arm64|x86_64) ;;
    *) echo "App Intents extraction requires a single-architecture binary, got: $ARCH" >&2; exit 1 ;;
esac
"$PROCESSOR" --toolchain-dir "$TOOLCHAIN" --sdk-root "$SDK" \
    --module-name LoomAppIntents --bundle-identifier dev.loomtty \
    --platform-family macOS --deployment-target 11.0 \
    --target-triple "$ARCH-apple-macos11.0" \
    --binary-file "$BINARY" --output "$APP/Contents/Resources" \
    --source-files "$SOURCE_DIR/Bridge.swift" "$SOURCE_DIR/Intents.swift" \
    --swift-const-vals "$CONST_VALUES" --compile-time-extraction
# The extractor can exit successfully after skipping a target. Such a bundle
# would have Swift code but no discoverable Shortcuts actions: fail the build.
METADATA="$APP/Contents/Resources/Metadata.appintents"
for FILE in extract.actionsdata version.json; do
    test -s "$METADATA/$FILE" || { echo "App Intents metadata was not generated: $FILE" >&2; exit 1; }
done
