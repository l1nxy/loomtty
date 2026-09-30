# Terminal benchmarks

## macOS comparison

The standard-library Python runner opens **real GUI instances** of loomtty,
Alacritty, Kitty and Ghostty, with isolated configs and a deterministic workload.
It records raw JSON and generates a Markdown comparison. Run from a logged-in
macOS graphical session with the windows visible, the machine on AC power, and
other CPU/GPU workloads stopped. Do not type into or resize benchmark windows.

```sh
# Explicit installation, only if needed; leaves existing apps alone.
bash bench/install_macos.sh
cargo build --release -p loomtty -p loomtty-server

# Verify exact grid size and startup without a performance run.
python3 bench/macos_bench.py --probe --rounds 1

# Full comparison: 3 independent launches per app, shuffled order.
python3 bench/macos_bench.py --rounds 3 --output /tmp/terminal-bench

# Small smoke test (do not use these noisy numbers for rankings).
python3 bench/macos_bench.py --rounds 1 --mib 0.25 --frames 30 \
  --latency-samples 20 --idle-seconds 0.5

python3 -m unittest discover -s bench -p 'test_*.py' -v
```

`--apps loomtty kitty` selects a subset. `--loom` and `--loom-server` select
specific builds. All other options are listed by `--help`. The output directory
must be new/empty, so previous runs cannot accidentally be reused as new results.
No Python packages or root permissions are required by the benchmark runner.

Latest comparison: [four terminals, three valid launches each (中文)](results/macos-comparison-2026-09-30/summary.zh-CN.md),
with [all metrics](results/macos-comparison-2026-09-30/report.md), raw data and logs.
This report preserves startup/build-interference failures and identifies the
supplemental runs. Read the Kitty latency/visibility caveat before comparing CPU.

Measured optimization: [2026-09-30 before/after results (中文)](results/macos-performance-2026-09-30/summary.zh-CN.md),
with [all metrics](results/macos-performance-2026-09-30/report.md) and raw logs/data.

CPU follow-up: [2026-09-30 row/glyph processing optimization (中文)](results/macos-cpu-2026-09-30/summary.zh-CN.md),
with paired before/after measurements at **2× DPI**, profiles and validation logs.
Its baseline already contains the earlier PTY scheduling and memory changes.
Do not compare its absolute CPU figures to the 1× reports above.

Historical example: [2026-09-30 comparison and later validity audit (中文)](results/macos-2026-09-30/summary.zh-CN.md),
with [full tables](results/macos-2026-09-30/report.md) and
[raw data](results/macos-2026-09-30/results.json). Its loomtty runs had background
watcher panics and are excluded from valid comparisons after the log audit.

### Compare a performance change

Save both binaries **before** changing/rebuilding the code, then use the paired
runner. It alternates before/after order each round, runs the same workload, and
rejects differing display/cell metrics. It records both client and server hashes.

```sh
mkdir -p /tmp/loom-before
cp target/release/loomtty target/release/loomtty-server /tmp/loom-before/
# Make the change, then rebuild.
cargo build --release -p loomtty -p loomtty-server
python3 bench/compare_loom.py --before /tmp/loom-before --rounds 3
```

For Menlo on a 1× display, use `--loom-height 512` for the default 100×30 grid;
on the calibrated 2× display the default is 496. Run a probe first and keep the
display unchanged. Never compare numbers from different DPI settings as though
they measured the same rendering workload.

### Workloads and measurements

| Workload | What it measures |
|---|---|
| Startup | Fresh app launch to first acknowledged cursor-position query; includes Python startup. loomtty includes private server startup. OS/file caches are warm. |
| Cold / warm / post-output idle | CPU, RSS and physical footprint before glyph warmup, after warmup, and after history population. |
| ASCII | Long plain-text lines, 8 MiB by default. |
| Truecolor | A different RGB SGR sequence for each character. |
| CJK / emoji | Complete UTF-8 wide-character sequences. |
| Scroll | Many short lines, stressing scrolling rather than glyph diversity. |
| Cursor | Cursor addressing, erase-line and full-grid redraw. |
| Idle DSR | 200 serial roundtrips, with a 5 ms pause between requests. |
| Loaded DSR | 200 roundtrips, each following at least 32 KiB of truecolor output. |
| Full / partial TUI | 300 updates at 60 Hz, mixed ASCII/CJK/emoji; entire viewport or one row. |
| Scrollback | 10,000 numbered main-screen lines, followed by an idle memory sample. |

Each bulk workload has a separate 64 KiB warmup. Payload generation is outside
the timed region; complete blocks are repeated, so actual byte counts can exceed
the requested size slightly. The exact byte count and SHA-256 are recorded.
Every run uses 100×30 cells and Menlo 10 pt, opaque background, disabled cursor
blink and ligatures, no shell profiles/integration. Font fallbacks, chrome,
render backends and glyph rasterizers remain native. Default loomtty window
calibration is 800×496 logical pixels (Menlo, 2× Retina); a different display
can use `--loom-width` / `--loom-height`. Any grid mismatch or later resize fails
the run rather than silently comparing different dimensions.

Bulk workloads run in the alternate screen to avoid scrollback-capacity bias.
The history test is separate: loomtty/Kitty/Alacritty are set to 10,000 lines,
while Ghostty uses a 10,000,000-byte cap including its visible screen. These are
different storage policies, not equivalent retained-history guarantees.

### Interpret the numbers correctly

The worker writes through `/dev/tty`, then sends DSR (`CSI 6 n`) and waits for
the expected response. This fences the terminal **parser**, not the GPU or
display. For loomtty, the server generates the response before the next
client-render update may happen. Do not interpret throughput or DSR latency as
FPS, visual latency, or proof that all intermediate frames were presented.
Input-to-photon testing requires a different instrumented or camera-based test.
TUI deadline misses are acknowledgement misses, not measured dropped frames.

macOS `proc_pid_rusage` samples CPU and memory every 20 ms. CPU Mach ticks are
converted with `mach_timebase_info` (important on Apple Silicon). CPU percentages
are relative to **one CPU core** and can exceed 100%. CPU includes a 250 ms tail
after each phase to catch queued render work. Idle CPU also includes the initial
DSR and that tail. RSS/footprint peaks are sampled peaks, not lifetime maxima.
The JSON preserves per-process components and measurement durations.

loomtty resources include **client + private server**; the others include their
GUI process. The common Python workload, launch shell, WindowServer and GPU
execution time are excluded. Shared pages can be double-counted in summed RSS;
physical footprint is reported alongside it rather than treating RSS as the
only memory metric. Medians and min–max ranges summarize independent launches,
not statistical confidence intervals. Native FPS caps, render coalescing and
occlusion policies still differ. Background load can materially change results.

### Isolation and cleanup

loomtty uses `LOOM_CONFIG_FILE`, `LOOM_STATE_DIR` and a private `TMPDIR` socket;
the user's remembered session/config is never overwritten. Configs live in a
dedicated, quiet `settings/` directory so config watchers never observe the
high-frequency result/handshake files. Competitors get
explicit config paths. Ghostty's native app executable is launched as a new owned
process with default config loading disabled. This also avoids a LaunchServices
activation issue observed with `open -n` when another instance is already open.
The worker's process ancestry verifies which instance owns the benchmark. Process identities are
checked before termination; there is no `killall` or system-wide preference edit.
All waits have timeouts; failures are reported and excluded from medians. A Rust
thread panic, loomtty config reload or DPI change in the log invalidates a run
even when the workload itself completed. Each measurement also checks for
concurrent `cargo`/`rustc` processes before and after the phase, and fails if a
build is found. This catches common interference, not every background workload;
keep other builds and performance tests stopped for the entire run. Ctrl-C
cleans up the active owned instance and leaves completed results on disk.

`report.md` is generated from `results.json`. Each run directory also preserves
config, logs, worker handshakes and phase files for troubleshooting. Machine,
versions, binary hashes, Git revision/dirty state, power and thermal status are
captured. Paths in raw results may identify the local checkout/user; inspect
before publishing them. `bench/results/` may hold selected checked-in reports.

### Primary references

- [Alacritty build/install](https://github.com/alacritty/alacritty/blob/v0.17.0/INSTALL.md)
- [Alacritty configuration](https://alacritty.org/config-alacritty.html)
- [Kitty configuration](https://sw.kovidgoyal.net/kitty/conf/)
- [Ghostty configuration reference](https://ghostty.org/docs/config/reference)
- [Apple process memory accounting](https://developer.apple.com/videos/play/wwdc2022/10106/)

## Earlier scripts

`termbench.sh`, `vtebench.sh`, `run_bench.sh` and `compare.py` are the older Linux
workflow. They use Linux tools and/or write-completion timing, and their results
must not be combined with this DSR-fenced macOS suite. The new runner does not
invoke them. `macos-memory.md` documents the earlier before/after optimization
experiment, which measured client-only memory and a different workload.
