#!/bin/bash
# Called by Cargo's macos-app-intents feature; metadata extraction happens at
# bundle time, after the Rust executable has linked this entire Swift archive.
set -euo pipefail
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="$1"
ARCH="$2"
SDK="$(xcrun --sdk macosx --show-sdk-path)"
mkdir -p "$OUTPUT_DIR"
xcrun --sdk macosx swiftc -swift-version 6 -parse-as-library \
    -target "$ARCH-apple-macos11.0" -sdk "$SDK" \
    -O -whole-module-optimization -module-name LoomAppIntents \
    -emit-library -static -o "$OUTPUT_DIR/libLoomAppIntents.a" \
    -emit-const-values-path "$OUTPUT_DIR/LoomAppIntents.swiftconstvalues" \
    -const-gather-protocols-list "$SOURCE_DIR/const-gather.json" \
    "$SOURCE_DIR/Bridge.swift" "$SOURCE_DIR/Intents.swift"
test -s "$OUTPUT_DIR/LoomAppIntents.swiftconstvalues"
