#!/usr/bin/env python3
"""Interleave before/after loomtty builds using the complete macOS benchmark.

python3 bench/compare_loom.py --before /tmp/loom-before --rounds 3
The before directory must contain loomtty and loomtty-server. --loom and
--loom-server select the after binaries; all workload flags match macos_bench.py.
"""

import copy
import datetime
import hashlib
import platform
import sys
import time

from macos_bench import ROOT, argument_parser, capture, report, run_one, save_json


def main():
    parser = argument_parser()
    parser.description = __doc__
    from pathlib import Path
    parser.add_argument("--before", required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("requires macOS")
    if args.probe:
        parser.error("run macos_bench.py --probe to calibrate first")
    if min(args.rounds, args.frames, args.latency_samples, args.mib, args.idle_seconds, args.timeout) <= 0:
        parser.error("counts and durations must be positive")
    if not 20 <= args.columns <= 250 or not 5 <= args.rows <= 100:
        parser.error("grid must be within 20..250 columns and 5..100 rows")
    args.output = args.output.expanduser().resolve()
    args.before = args.before.expanduser().resolve()
    args.loom, args.loom_server = args.loom.resolve(), args.loom_server.resolve()
    if args.output.exists() and any(args.output.iterdir()):
        parser.error("output directory must be new or empty")
    variants = {"before": copy.copy(args), "after": copy.copy(args)}
    variants["before"].loom = args.before / "loomtty"
    variants["before"].loom_server = args.before / "loomtty-server"
    versions = {}
    for name, variant in variants.items():
        for binary in (variant.loom, variant.loom_server):
            if not binary.is_file():
                parser.error(f"missing {binary}")
        versions[name] = dict(
            text=capture([variant.loom, "--version"]),
            sha256=hashlib.sha256(variant.loom.read_bytes()).hexdigest(),
            server_sha256=hashlib.sha256(variant.loom_server.read_bytes()).hexdigest(),
        )
    args.output.mkdir(parents=True, exist_ok=True)
    data = dict(
        schema_version=1, created_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        machine=dict(chip=capture(["sysctl", "-n", "machdep.cpu.brand_string"]),
                     macos=capture(["sw_vers", "-productVersion"]),
                     architecture=platform.machine(),
                     memory_bytes=int(capture(["sysctl", "-n", "hw.memsize"])),
                     power=capture(["pmset", "-g", "batt"]), thermal=capture(["pmset", "-g", "therm"])),
        settings={**{k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
                  "apps": ["before", "after"]}, versions=versions,
        git_head=capture(["git", "-C", ROOT, "rev-parse", "HEAD"]),
        git_status=capture(["git", "-C", ROOT, "status", "--short"]),
        benchmark_sha256={name: hashlib.sha256((ROOT / "bench" / name).read_bytes()).hexdigest()
                          for name in ("macos_bench.py", "terminal_workload.py", "compare_loom.py")},
        runs=[],
    )
    expected_display = None
    try:
        for round_index in range(args.rounds):
            # Reverse order every round to reduce systematic drift bias.
            order = ("before", "after") if round_index % 2 == 0 else ("after", "before")
            for name in order:
                print(f"Round {round_index + 1}/{args.rounds}: {name}", flush=True)
                run = run_one("loomtty", round_index, args.output / f"r{round_index}-{name}", variants[name])
                run["app"] = name
                if "error" not in run:
                    if expected_display is None:
                        expected_display = run.get("display")
                    if not run.get("display") or run["display"] != expected_display:
                        run["error"] = "display/cell metrics differ between before/after runs"
                data["runs"].append(run)
                save_json(args.output / "results.json", data)
                report(data, args.output)
                if "error" in run:
                    print(f"FAILED: {run['error']}", flush=True)
                    return 1
                time.sleep(1)
    finally:
        save_json(args.output / "results.json", data)
        report(data, args.output)
    print(f"Report: {args.output / 'report.md'}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
