#!/usr/bin/env bash
# Launch fresh ciri and ghostty instances, run benchmark in each,
# then compare results.
#
# Usage: ./run_bench.sh

set -euo pipefail

BENCH_SCRIPT="$(cd "$(dirname "$0")" && pwd)/termbench.sh"
RESULT_CIRI="/tmp/termbench_ciri.txt"
RESULT_GHOSTTY="/tmp/termbench_ghostty.txt"

chmod +x "$BENCH_SCRIPT"

echo "=== Terminal Benchmark Runner ==="
echo ""

# Clean old results
rm -f "$RESULT_CIRI" "$RESULT_GHOSTTY"

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

# ---- Ciri ----
echo "[2/4] Launching fresh ciri instance with benchmark..."
# Create a new ciri session and run bench inside it
SESSION_NAME="bench_$(date +%s)"
ciri new &
CIRI_PID=$!
sleep 3  # wait for ciri to start

# Run the benchmark via msg run-command
ciri msg run-command "$SESSION_NAME" "bash '$BENCH_SCRIPT' ciri"
echo "  Ciri session: $SESSION_NAME"
echo "  Waiting for ciri bench to complete..."

while [ ! -f "$RESULT_CIRI" ] || ! grep -q "Benchmark Complete" "$RESULT_CIRI" 2>/dev/null; do
    sleep 2
done
echo "  Ciri benchmark done."

# Kill ciri bench session
ciri kill "$SESSION_NAME" 2>/dev/null || true
kill $CIRI_PID 2>/dev/null || true
sleep 1
echo ""

# ---- Compare ----
echo "[3/4] Comparing results..."
echo ""

python3 "$(dirname "$0")/compare.py" "$RESULT_CIRI" "$RESULT_GHOSTTY"
