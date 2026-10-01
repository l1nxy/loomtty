#!/bin/bash
# Foundation-only tests with an in-process fake host; no UI or daemon is opened.
set -euo pipefail
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/loom-intents-tests.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
xcrun swiftc -swift-version 6 -parse-as-library -module-name LoomAppIntents \
    "$SOURCE_DIR/Bridge.swift" "$SOURCE_DIR/Intents.swift" "$SOURCE_DIR/BridgeTests.swift" \
    -o "$TEMP_DIR/tests"
"$TEMP_DIR/tests"

# The async Swift test main already owns MainActor. Check the actual native
# entry-point contract separately, with Swift loaded into a plain C main().
"$SOURCE_DIR/build.sh" "$TEMP_DIR" "$(uname -m)"
xcrun clang -c -Wall -Wextra -Werror "$SOURCE_DIR/BridgeABI.c" -o "$TEMP_DIR/abi.o"
xcrun swiftc "$TEMP_DIR/abi.o" "$TEMP_DIR/libLoomAppIntents.a" -o "$TEMP_DIR/abi"
"$TEMP_DIR/abi"
