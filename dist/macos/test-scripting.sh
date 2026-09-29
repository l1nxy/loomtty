#!/bin/bash
# Validate the real sdef/Cocoa contract without starting NSApplication or PTYs.
# Foundation reads scripting dictionaries from the process's main bundle, so
# run the dedicated Rust unit test inside a temporary .app test harness.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/loom-scripting.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT
cd "$REPO_ROOT"
if ! cargo test -p loomtty --lib --no-run --message-format=json > "$TEST_DIR/build.json"; then
    python3 - "$TEST_DIR/build.json" <<'PYERROR'
import json, sys
for line in open(sys.argv[1]):
    value = json.loads(line)
    if value.get('reason') == 'compiler-message':
        print(value['message'].get('rendered', ''), file=sys.stderr)
PYERROR
    exit 1
fi
TEST_BIN="$(python3 - "$TEST_DIR/build.json" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    value = json.loads(line)
    if value.get('reason') == 'compiler-artifact' and value.get('executable') and value['target']['name'] == 'loomtty':
        print(value['executable'])
PY
)"
[ -n "$TEST_BIN" ] || { echo "test executable not found" >&2; exit 1; }
TEST_APP="$TEST_DIR/ScriptingTests.app"
mkdir -p "$TEST_APP/Contents/MacOS" "$TEST_APP/Contents/Resources"
install -m755 "$TEST_BIN" "$TEST_APP/Contents/MacOS/ScriptingTests"
cp "$SCRIPT_DIR/loomtty.sdef" "$TEST_APP/Contents/Resources/loomtty.sdef"
python3 - "$TEST_APP/Contents/Info.plist" <<'PY'
import plistlib, sys
with open(sys.argv[1], 'wb') as output:
    plistlib.dump({
        'CFBundleIdentifier': 'dev.loomtty.scripting-tests',
        'CFBundleName': 'ScriptingTests',
        'CFBundleExecutable': 'ScriptingTests',
        'CFBundlePackageType': 'APPL',
        'NSAppleScriptEnabled': True,
        'OSAScriptingDefinition': 'loomtty.sdef',
    }, output)
PY
LOOM_SCRIPTING_TEST_BUNDLE=1 "$TEST_APP/Contents/MacOS/ScriptingTests" \
    --ignored --exact macos::scripting::cocoa::tests::bundled_dictionary_matches_cocoa_objects_and_commands \
    --nocapture --test-threads=1
