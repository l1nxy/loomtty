#!/usr/bin/env bash
# Launch fresh loomtty and ghostty instances, run benchmark in each,
# then compare results.
#
# Usage: ./run_bench.sh

set -euo pipefail

BENCH_SCRIPT="$(cd "$(dirname "$0")" && pwd)/termbench.sh"
RESULT_LOOM="/tmp/termbench_loom.txt"
RESULT_GHOSTTY="/tmp/termbench_ghostty.txt"

chmod +x "$BENCH_SCRIPT"

echo "=== Terminal Benchmark Runner ==="
echo ""

# Clean old results
rm -f "$RESULT_LOOM" "$RESULT_GHOSTTY"

# ---- Ghostty ----
echo "[1/4] Launching fresh ghostty instance with benchmark..."
ghostty -e bash -c "'$BENCH_SCRIPT' ghostty; echo 'Press Enter to close'; read" &
GHOSTTY_PID=$!
echo "  Ghostty launched (PID: $GHOSTTY_PID)"
echo "  Waiting for ghostty bench to complete..."

# Wait for result file
while [ ! -f "$RESULT_GHOSTTY" ] || ! grep -q "Benchmark Complete" "$RESULT_GHOSTTY" 2>/dev/null; do
    sleep 2
done
echo "  Ghostty benchmark done."

# Kill ghostty
kill $GHOSTTY_PID 2>/dev/null || true
sleep 1
echo ""

# ---- Loomtty ----
echo "[2/4] Launching fresh loomtty instance with benchmark..."
# Create a new loomtty session and run bench inside it
SESSION_NAME="bench_$(date +%s)"
loomtty new &
LOOM_PID=$!
sleep 3  # wait for loomtty to start

# Run the benchmark via msg run-command
loomtty msg run-command "$SESSION_NAME" "bash '$BENCH_SCRIPT' loomtty"
echo "  Loomtty session: $SESSION_NAME"
echo "  Waiting for loomtty bench to complete..."

while [ ! -f "$RESULT_LOOM" ] || ! grep -q "Benchmark Complete" "$RESULT_LOOM" 2>/dev/null; do
    sleep 2
done
echo "  Loomtty benchmark done."

# Kill loomtty bench session
loomtty kill "$SESSION_NAME" 2>/dev/null || true
kill $LOOM_PID 2>/dev/null || true
sleep 1
echo ""

# ---- Compare ----
echo "[3/4] Comparing results..."
echo ""

python3 "$(dirname "$0")/compare.py" "$RESULT_LOOM" "$RESULT_GHOSTTY"
