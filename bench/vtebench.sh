#!/usr/bin/env bash
# Run vtebench (Alacritty's standard terminal benchmark) inside the current
# terminal. Must be invoked from a loomtty (or comparison) window — it writes
# benchmark data to stdout and measures wall-clock to consume it.
#
# Usage:
#   bench/vtebench.sh                  # default: 10s/benchmark
#   bench/vtebench.sh --max-secs 5     # shorter
#   bench/vtebench.sh --dat out.dat    # gnuplot output

set -euo pipefail

VTEBENCH_DIR="${VTEBENCH_DIR:-/tmp/vtebench}"
VTEBENCH_BIN="$VTEBENCH_DIR/target/release/vtebench"

if [ ! -x "$VTEBENCH_BIN" ]; then
    echo "Building vtebench at $VTEBENCH_DIR ..."
    if [ ! -d "$VTEBENCH_DIR" ]; then
        git clone --depth 1 https://github.com/alacritty/vtebench.git "$VTEBENCH_DIR"
    fi
    (cd "$VTEBENCH_DIR" && cargo build --release)
fi

echo "Terminal: ${TERM:-?}  Size: $(stty size 2>/dev/null || echo '?')"
echo "Running vtebench (this writes a lot of garbage to your screen) ..."
echo

"$VTEBENCH_BIN" -b "$VTEBENCH_DIR/benchmarks" "$@"
