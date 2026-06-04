#!/usr/bin/env bash
# Terminal emulator benchmark suite
# Usage: ./termbench.sh [output_label]
# Results are written to /tmp/termbench_<label>.txt

set -euo pipefail

LABEL="${1:-unknown}"
RESULT_FILE="/tmp/termbench_${LABEL}.txt"
DATA_DIR="/tmp/termbench_data"
mkdir -p "$DATA_DIR"

# Resolve terminal PID — walk up from our shell to find the terminal emulator
find_terminal_pid() {
    local pid=$$
    while [ "$pid" -gt 1 ]; do
        pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
        local comm=$(ps -o comm= -p "$pid" 2>/dev/null || echo "")
        case "$comm" in
            loomtty|loomtty-server|ghostty|alacritty|kitty|wezterm*|foot)
                echo "$pid"
                return
                ;;
        esac
    done
    # fallback: direct parent
    ps -o ppid= -p $$ | tr -d ' '
}

TERM_PID=$(find_terminal_pid)
TERM_NAME=$(ps -o comm= -p "$TERM_PID" 2>/dev/null || echo "unknown")

# Helper: high-res timer in milliseconds
now_ms() {
    python3 -c "import time; print(int(time.monotonic_ns() // 1_000_000))"
}

# ============================================================
# Generate test data (cached in /tmp)
# ============================================================
generate_data() {
    echo "Generating test data..."

    if [ ! -f "$DATA_DIR/plain_10mb.txt" ]; then
        python3 -c "
import random, string
data = ''.join(random.choices(string.ascii_letters + string.digits + ' \n', k=10*1024*1024))
print(data, end='')
" > "$DATA_DIR/plain_10mb.txt"
    fi

    if [ ! -f "$DATA_DIR/sgr_5mb.txt" ]; then
        python3 -c "
import random
lines = []
for i in range(50000):
    fg = random.randint(0, 255)
    bg = random.randint(0, 255)
    bold = '\033[1m' if random.random() > 0.5 else ''
    lines.append(f'{bold}\033[38;5;{fg}m\033[48;5;{bg}m' + 'A' * random.randint(20, 80) + '\033[0m')
print('\n'.join(lines), end='')
" > "$DATA_DIR/sgr_5mb.txt"
    fi

    if [ ! -f "$DATA_DIR/cjk_5mb.txt" ]; then
        python3 -c "
import random
cjk_ranges = list(range(0x4E00, 0x9FFF)) + list(range(0x3040, 0x309F)) + list(range(0xAC00, 0xD7AF))
lines = []
for i in range(40000):
    line = ''.join(chr(random.choice(cjk_ranges)) for _ in range(60))
    lines.append(line)
print('\n'.join(lines), end='')
" > "$DATA_DIR/cjk_5mb.txt"
    fi

    if [ ! -f "$DATA_DIR/scroll_lines.txt" ]; then
        seq 1 200000 > "$DATA_DIR/scroll_lines.txt"
    fi

    if [ ! -f "$DATA_DIR/mixed_stress.txt" ]; then
        python3 -c "
import random
cjk = list(range(0x4E00, 0x9FFF))
lines = []
for i in range(30000):
    fg = random.randint(0, 255)
    cjk_part = ''.join(chr(random.choice(cjk)) for _ in range(20))
    ascii_part = ''.join(random.choices('abcdefghijklmnop', k=20))
    lines.append(f'\033[38;5;{fg}m\033[1m{ascii_part}\033[0m {cjk_part}')
print('\n'.join(lines), end='')
" > "$DATA_DIR/mixed_stress.txt"
    fi

    echo "Done."
}

# ============================================================
# Benchmark: throughput
# Cat file to /dev/tty so terminal actually renders it.
# Measure wall-clock time. Run 3 times, report average.
# ============================================================
bench_throughput() {
    local file=$1 label=$2
    local times=()

    # Warmup run
    cat "$file" > /dev/tty 2>/dev/null

    for i in 1 2 3; do
        local start=$(now_ms)
        cat "$file" > /dev/tty 2>/dev/null
        local end=$(now_ms)
        times+=( $((end - start)) )
    done

    local sum=0
    for t in "${times[@]}"; do sum=$((sum + t)); done
    local avg=$((sum / 3))
    local size_bytes
    size_bytes=$(stat --printf="%s" "$file")
    local throughput
    throughput=$(python3 -c "
avg_s = $avg / 1000
sz = $size_bytes
print(f'{sz / avg_s / 1048576:.2f}') if avg_s > 0 else print('inf')
")
    local size_mb
    size_mb=$(python3 -c "print(f'{$size_bytes / 1048576:.2f}')")
    echo "  $label: avg=${avg}ms, size=${size_mb}MB, throughput=${throughput} MB/s  [${times[0]}, ${times[1]}, ${times[2]}]"
}

# ============================================================
# Benchmark: idle CPU
# ============================================================
bench_idle_cpu() {
    local pid=$1
    echo "  Measuring idle CPU for PID $pid ($TERM_NAME) over 10 seconds..."
    local result
    result=$(pidstat -p "$pid" 1 10 2>/dev/null | grep 'Average' | awk '{print $8}')
    echo "  Idle CPU: ${result}%"
}

# ============================================================
# Benchmark: CPU under rendering load
# Start pidstat, then blast content to terminal, collect avg CPU.
# ============================================================
bench_cpu_under_load() {
    local pid=$1 file=$2 label=$3
    local tmpfile="/tmp/termbench_cpu_${label}_$$"

    echo "  Measuring CPU during '$label' render..."
    pidstat -p "$pid" 1 20 > "$tmpfile" 2>/dev/null &
    local pidstat_pid=$!
    sleep 1

    for i in 1 2 3 4 5; do
        cat "$file" > /dev/tty 2>/dev/null
    done

    sleep 2
    wait $pidstat_pid 2>/dev/null || true
    local result
    result=$(grep 'Average' "$tmpfile" | awk '{print $8}')
    echo "  CPU during $label: ${result}%"
    rm -f "$tmpfile"
}

# ============================================================
# Main — write only metrics to result file
# ============================================================
main() {
    echo "=== Terminal Benchmark Results ==="
    echo "Label: $LABEL"
    echo "Terminal: $TERM_NAME (PID: $TERM_PID)"
    echo "Date: $(date)"
    echo "Kernel: $(uname -r)"
    echo ""

    generate_data
    echo ""

    echo "--- 1. Idle CPU (10s sample) ---"
    sleep 2
    bench_idle_cpu "$TERM_PID"
    echo ""

    echo "--- 2. Throughput: Plain Text (10MB) ---"
    bench_throughput "$DATA_DIR/plain_10mb.txt" "plain_10mb"
    echo ""

    echo "--- 3. Throughput: SGR Colored (5MB) ---"
    bench_throughput "$DATA_DIR/sgr_5mb.txt" "sgr_colored"
    echo ""

    echo "--- 4. Throughput: CJK/Unicode (5MB) ---"
    bench_throughput "$DATA_DIR/cjk_5mb.txt" "cjk_unicode"
    echo ""

    echo "--- 5. Throughput: Scrollback (200k lines) ---"
    bench_throughput "$DATA_DIR/scroll_lines.txt" "scroll_200k"
    echo ""

    echo "--- 6. Throughput: Mixed Stress (SGR+CJK) ---"
    bench_throughput "$DATA_DIR/mixed_stress.txt" "mixed_stress"
    echo ""

    echo "--- 7. CPU Under Load: Plain Text ---"
    bench_cpu_under_load "$TERM_PID" "$DATA_DIR/plain_10mb.txt" "plain_text"
    echo ""

    echo "--- 8. CPU Under Load: SGR Colored ---"
    bench_cpu_under_load "$TERM_PID" "$DATA_DIR/sgr_5mb.txt" "sgr_colored"
    echo ""

    echo "--- 9. CPU Under Load: CJK ---"
    bench_cpu_under_load "$TERM_PID" "$DATA_DIR/cjk_5mb.txt" "cjk"
    echo ""

    echo "--- 10. Memory Usage ---"
    local rss swap
    rss=$(grep VmRSS /proc/$TERM_PID/status 2>/dev/null | awk '{print $2}')
    swap=$(grep VmSwap /proc/$TERM_PID/status 2>/dev/null | awk '{print $2}')
    echo "  RSS: ${rss} kB"
    echo "  Swap: ${swap} kB"
    echo "  Total: $(( rss + swap )) kB"
    echo ""

    echo "=== Benchmark Complete ==="
}

main 2>&1 | tee "$RESULT_FILE"
echo ""
echo "Results saved to: $RESULT_FILE"
