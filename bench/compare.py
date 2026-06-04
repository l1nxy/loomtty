#!/usr/bin/env python3
"""Compare two termbench result files and print a table."""

import re
import sys

def parse_results(filepath):
    results = {}
    with open(filepath) as f:
        text = f.read()

    # Throughput lines: "  label: avg=123ms, size=10.00MB, throughput=45.67 MB/s"
    for m in re.finditer(r'(\w+): avg=(\d+)ms, size=([\d.]+)MB, throughput=([\d.inf]+) MB/s', text):
        label, avg_ms, size_mb, throughput = m.groups()
        results[f"throughput_{label}"] = {
            "avg_ms": int(avg_ms),
            "throughput": float(throughput) if throughput != "inf" else float("inf"),
            "size_mb": float(size_mb),
        }

    # Idle CPU: "  Idle CPU: 1.23%"
    m = re.search(r'Idle CPU: ([\d.]+)%', text)
    if m:
        results["idle_cpu"] = float(m.group(1))

    # CPU under load: "  CPU during label: 1.23%"
    for m in re.finditer(r'CPU during ([\w_]+): ([\d.]+)%', text):
        results[f"cpu_{m.group(1)}"] = float(m.group(2))

    # Memory: RSS, Swap, Total
    m = re.search(r'RSS: (\d+) kB', text)
    if m:
        results["rss_kb"] = int(m.group(1))
    m = re.search(r'Swap: (\d+) kB', text)
    if m:
        results["swap_kb"] = int(m.group(1))
    m = re.search(r'Total: (\d+) kB', text)
    if m:
        results["mem_total_kb"] = int(m.group(1))

    return results


def fmt_winner(a, b, lower_is_better=True):
    """Return colored indicator: green for winner, red for loser."""
    if a == b:
        return "  ", "  "
    if lower_is_better:
        return (" ✓" if a < b else "  "), (" ✓" if b < a else "  ")
    else:
        return (" ✓" if a > b else "  "), (" ✓" if b > a else "  ")


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <loom_results> <ghostty_results>")
        sys.exit(1)

    loom = parse_results(sys.argv[1])
    ghostty = parse_results(sys.argv[2])

    print("=" * 72)
    print(f"{'Benchmark':<30} {'loom':>15} {'ghostty':>15}  {'diff':>8}")
    print("=" * 72)

    # Throughput tests
    for key in ["throughput_plain_10mb", "throughput_sgr_colored",
                "throughput_cjk_unicode", "throughput_scroll_200k",
                "throughput_mixed_stress"]:
        label = key.replace("throughput_", "")
        if key in loom and key in ghostty:
            c_tp = loom[key]["throughput"]
            g_tp = ghostty[key]["throughput"]
            c_ms = loom[key]["avg_ms"]
            g_ms = ghostty[key]["avg_ms"]
            diff = ((g_ms - c_ms) / g_ms * 100) if g_ms > 0 else 0
            sign = "+" if diff > 0 else ""
            print(f"  {label + ' (MB/s)':<28} {c_tp:>12.1f}   {g_tp:>12.1f}   {sign}{diff:>5.1f}%")
            print(f"  {label + ' (ms)':<28} {c_ms:>12d}   {g_ms:>12d}")

    print("-" * 72)

    # CPU
    if "idle_cpu" in loom and "idle_cpu" in ghostty:
        c, g = loom["idle_cpu"], ghostty["idle_cpu"]
        print(f"  {'Idle CPU (%)':<28} {c:>12.1f}   {g:>12.1f}")

    for key in ["cpu_plain_text", "cpu_sgr_colored", "cpu_cjk"]:
        label = key.replace("cpu_", "CPU: ")
        if key in loom and key in ghostty:
            c, g = loom[key], ghostty[key]
            print(f"  {label + ' (%)':<28} {c:>12.1f}   {g:>12.1f}")

    print("-" * 72)

    # Memory
    if "rss_kb" in loom and "rss_kb" in ghostty:
        c, g = loom["rss_kb"] / 1024, ghostty["rss_kb"] / 1024
        print(f"  {'RSS (MB)':<28} {c:>12.1f}   {g:>12.1f}")
    if "mem_total_kb" in loom and "mem_total_kb" in ghostty:
        c, g = loom["mem_total_kb"] / 1024, ghostty["mem_total_kb"] / 1024
        print(f"  {'RSS+Swap (MB)':<28} {c:>12.1f}   {g:>12.1f}")

    print("=" * 72)


if __name__ == "__main__":
    main()
