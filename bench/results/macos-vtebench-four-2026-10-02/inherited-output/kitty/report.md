# macOS terminal benchmark

Generated: 2026-10-02T07:59:39.505413+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | kitty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 822.5 (822.5–822.5) |
| Cold idle RSS (MiB) ↓ | 126.0 |
| Cold idle physical footprint (MiB) ↓ | 58.6 |
| Cold idle CPU (% of one core) ↓ | 0.00 |
| Warm idle RSS (MiB) ↓ | 134.5 |
| Warm idle physical footprint (MiB) ↓ | 64.2 |
| Warm idle CPU (% of one core) ↓ | 0.01 |
| After history RSS (MiB) ↓ | 168.7 |
| After history physical footprint (MiB) ↓ | 98.1 |
| After history CPU (% of one core) ↓ | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 210.2 (210.2–210.2) |
| ascii terminal CPU (s) ↓ | 0.035 |
| truecolor parser throughput (MiB/s) ↑ | 171.5 (171.5–171.5) |
| truecolor terminal CPU (s) ↓ | 0.044 |
| cjk parser throughput (MiB/s) ↑ | 175.5 (175.5–175.5) |
| cjk terminal CPU (s) ↓ | 0.043 |
| emoji parser throughput (MiB/s) ↑ | 191.3 (191.3–191.3) |
| emoji terminal CPU (s) ↓ | 0.037 |
| scroll parser throughput (MiB/s) ↑ | 62.4 (62.4–62.4) |
| scroll terminal CPU (s) ↓ | 0.125 |
| cursor parser throughput (MiB/s) ↑ | 189.8 (189.8–189.8) |
| cursor terminal CPU (s) ↓ | 0.039 |
| latency DSR p50 (ms) ↓ | 3.524 |
| latency DSR p95 (ms) ↓ | 3.604 |
| latency DSR p99 (ms) ↓ | 3.637 |
| loaded_latency DSR p50 (ms) ↓ | 3.369 |
| loaded_latency DSR p95 (ms) ↓ | 3.587 |
| loaded_latency DSR p99 (ms) ↓ | 3.668 |
| tui_full DSR p50 (ms) ↓ | 3.263 |
| tui_full DSR p95 (ms) ↓ | 3.374 |
| tui_full DSR p99 (ms) ↓ | 3.490 |
| tui_full terminal CPU (%) ↓ | 13.0 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 3.567 |
| tui_partial DSR p95 (ms) ↓ | 3.627 |
| tui_partial DSR p99 (ms) ↓ | 3.666 |
| tui_partial terminal CPU (%) ↓ | 9.0 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 154.4 (154.4–154.4) |
| vtebench cursor_motion CPU ms/MiB ↓ | 6.824 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 94.1 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 185.7 (185.7–185.7) |
| vtebench dense_cells CPU ms/MiB ↓ | 5.506 |
| vtebench dense_cells physical footprint (MiB) ↓ | 94.2 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 242.7 (242.7–242.7) |
| vtebench light_cells CPU ms/MiB ↓ | 3.721 |
| vtebench light_cells physical footprint (MiB) ↓ | 94.3 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 146.9 (146.9–146.9) |
| vtebench medium_cells CPU ms/MiB ↓ | 7.228 |
| vtebench medium_cells physical footprint (MiB) ↓ | 95.0 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 16.8 (16.8–16.8) |
| vtebench scrolling CPU ms/MiB ↓ | 61.642 |
| vtebench scrolling physical footprint (MiB) ↓ | 95.8 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 42.3 (42.3–42.3) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 24.823 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 95.8 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 42.6 (42.6–42.6) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 24.784 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 95.8 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 166.7 (166.7–166.7) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 6.101 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 95.9 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 41.9 (41.9–41.9) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 25.109 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 95.9 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 42.8 (42.8–42.8) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 24.715 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 95.9 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 90.1 (90.1–90.1) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 11.629 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 95.9 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 134.1 (134.1–134.1) |
| vtebench unicode CPU ms/MiB ↓ | 9.031 |
| vtebench unicode physical footprint (MiB) ↓ | 621.3 |
| Peak sampled RSS (MiB) ↓ | 714.1 |
| Peak sampled physical footprint (MiB) ↓ | 621.9 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- Upstream vtebench: `ead80032e57dee2e75f0b51f2ea67528647d9944`; binary SHA-256 `901e9ec14c457ade9afb265b6fc31f9b9139cfd1c99a9f86cb095ea463a3fb70`
- vtebench is unmodified; BSD ps tty padding is trimmed only while generating payloads. All payload/setup bytes are hashed and frozen before measurement.
- Official timing measures blocked PTY writes with millisecond truncation, not parser completion or displayed FPS. It uses 16 MiB minimum samples, one warmup, and the configured time limit.
- CPU ms/MiB includes setup, warmup and drain/settling; its denominator counts payload bytes including warmup, excluding small setup/reset/status bytes.
- PTY output modes (mode, flags): [('original', '3')]. Missing legacy metadata means raw output, but the exact flags were not recorded. Payload byte counts precede kernel output processing.
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
