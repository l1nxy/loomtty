# Alacritty vtebench + macOS resource comparison

The macOS runner can run the **unmodified upstream Rust vtebench harness** and
all 12 default workloads alongside its DSR/TUI/history suite:

```sh
git clone https://github.com/alacritty/vtebench /tmp/vtebench
(cd /tmp/vtebench && cargo build --release)
python3 bench/macos_bench.py --probe --rounds 1 --vtebench /tmp/vtebench \
  --loom-width 600 --loom-height 400
python3 bench/macos_bench.py --rounds 3 --frames 600 --vtebench /tmp/vtebench \
  --vtebench-seconds 3 --confirm-visible --loom-width 600 --loom-height 400 \
  --output /tmp/four-terminal-vtebench
```

Build everything before measurements. The checkout must be clean; results record
its commit, harness binary hash, wrapper hashes, every setup/payload hash, and
all integer-millisecond upstream samples. The real PTY's grid must match.

`--confirm-visible` pauses each fresh instance after preparation and writes
`rN-APP/needs-visibility.json`. Bring that exact benchmark window to the front
and verify it is visible (for an agent, use CUA). Then write `visible.json` in the
same directory, e.g. `{"method":"CUA Raise + screenshot","pid":1234}`. The runner
records this receipt and waits at most five minutes. Do not interact with or
cover the window during the following measurements. This is an operator
confirmation, not continuous visibility instrumentation; later occlusion remains
a limitation. Without the option, ensure visibility yourself.

Upstream generators use `ps -o tty=` to build a device path. BSD ps adds a trailing
space, so preparation places a private shim on PATH that trims **only** the tty
query. No global executable or upstream file is changed. Generator failures,
empty payloads and stderr fail the run instead of being silently skipped.
Generators run in the actual benchmark PTY; their exact bytes are frozen in
per-run executable `cat` wrappers before timing. These are passed to upstream
vtebench. A cross-app mismatch invalidates the run. Use `--probe` first to check
all manifests without measurements.

Each upstream phase uses 16 MiB minimum payload samples (to reduce millisecond
quantization), one warmup, and the selected seconds of measurement. The official
harness retains its resets, setup and write-blocking timer. We do not change it
to DSR timing. A DSR fence after the entire phase drains terminal parsing before
final CPU/memory sampling; CPU includes the warmup and 250 ms settling tail.

Report throughput is sample bytes / median upstream sample milliseconds;
CPU ms/MiB counts terminal CPU divided by measured-plus-warmup payload bytes.
Small setup/reset/status bytes are excluded from that denominator. CPU and
memory exclude the workload, generators, WindowServer and GPU execution; Loomtty
includes both GUI and server. Report RSS and physical footprint separately.

The upstream test measures **PTY write/read throughput**, not display FPS or
input-to-photon latency. A terminal can coalesce most of this flood. Keep its
results separate from DSR latency and fixed-rate TUI CPU. Native scrolling limits
and render policies also differ. See the upstream [README](https://github.com/alacritty/vtebench)
and [timing implementation](https://github.com/alacritty/vtebench/blob/master/src/bench.rs).

## PTY output mode

The DSR/TUI driver uses raw mode, but upstream vtebench does not change termios.
Official phases now restore the output flags captured before entering raw mode;
raw input remains enabled for DSR responses outside the timed subprocess. Each
phase records the actual output flags, OPOST and ONLCR. The raw settings are
restored even when the upstream process fails. This matters for workloads such
as scrolling (`y\n`): ONLCR can turn LF into CRLF in the kernel and change which
columns the emulator writes and clears.

`--vtebench-output-mode raw` reproduces the earlier raw-PTY variant. Reports
without an `output_mode` field were raw and must not be mixed with the new
default for throughput comparisons. Their fixed-rate TUI and memory phases
are unchanged. Byte denominators are upstream payload bytes before PTY output
processing, matching the upstream harness, not bytes after kernel translation.
