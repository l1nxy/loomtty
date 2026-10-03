#!/usr/bin/env python3
"""Cross-check the benchmark's libproc clock against a child process CPU clock.

The child stays alive after each workload so proc_pid_rusage can be sampled.
This checks units/accounting, not the repeatability of GUI performance.
"""

import argparse
import json
import subprocess
import sys
import time

from macos_bench import Process


def worker():
    print("ready", flush=True)
    for line in sys.stdin:
        mode, duration = json.loads(line)
        start = time.process_time()
        if mode == "busy":
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                pass
        elif mode == "sleep":
            time.sleep(duration)
        else:
            raise ValueError(mode)
        print(json.dumps(time.process_time() - start), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        worker()
        return 0
    if sys.platform != "darwin":
        parser.error("requires macOS")
    records = []
    with subprocess.Popen(
        [sys.executable, __file__, "--worker"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
    ) as child:
        try:
            assert child.stdout.readline().strip() == "ready"
            process = Process(child.pid, "clock-check")
            for mode in ("sleep", "busy", "busy", "busy", "sleep"):
                counters_before = process.counters()
                before = process.read()["cpu_seconds"]
                child.stdin.write(json.dumps([mode, 2.0]) + "\n")
                child.stdin.flush()
                independent = json.loads(child.stdout.readline())
                observed = process.read()["cpu_seconds"] - before
                counters_after = process.counters()
                # Parent samples also include tiny command/report overhead.
                tolerance = max(0.005, independent * 0.01)
                records.append(dict(
                    mode=mode, child_process_time_seconds=independent,
                    benchmark_cpu_seconds=observed,
                    difference_seconds=observed - independent,
                    tolerance_seconds=tolerance,
                    passed=abs(observed - independent) <= tolerance,
                    hardware_counters=(
                        {k: counters_after[k] - counters_before[k] for k in counters_before}
                        if counters_before is not None and counters_after is not None else None
                    ),
                ))
        finally:
            child.stdin.close()
            child.wait(timeout=10)
    passed = all(record["passed"] for record in records)
    print(json.dumps(dict(passed=passed, samples=records), indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
