# macOS terminal benchmark

Generated: 2026-10-02T03:49:20.702755+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 708.2 (708.2–708.2) |
| Cold idle RSS (MiB) ↓ | 118.4 |
| Cold idle physical footprint (MiB) ↓ | 63.2 |
| Cold idle CPU (% of one core) ↓ | 0.05 |
| Warm idle RSS (MiB) ↓ | 125.2 |
| Warm idle physical footprint (MiB) ↓ | 70.5 |
| Warm idle CPU (% of one core) ↓ | 0.03 |
| After history RSS (MiB) ↓ | 201.7 |
| After history physical footprint (MiB) ↓ | 147.7 |
| After history CPU (% of one core) ↓ | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 254.0 (254.0–254.0) |
| ascii terminal CPU (s) ↓ | 0.045 |
| truecolor parser throughput (MiB/s) ↑ | 302.1 (302.1–302.1) |
| truecolor terminal CPU (s) ↓ | 0.041 |
| cjk parser throughput (MiB/s) ↑ | 318.5 (318.5–318.5) |
| cjk terminal CPU (s) ↓ | 0.039 |
| emoji parser throughput (MiB/s) ↑ | 347.4 (347.4–347.4) |
| emoji terminal CPU (s) ↓ | 0.037 |
| scroll parser throughput (MiB/s) ↑ | 215.1 (215.1–215.1) |
| scroll terminal CPU (s) ↓ | 0.050 |
| cursor parser throughput (MiB/s) ↑ | 252.2 (252.2–252.2) |
| cursor terminal CPU (s) ↓ | 0.045 |
| latency DSR p50 (ms) ↓ | 0.076 |
| latency DSR p95 (ms) ↓ | 0.131 |
| latency DSR p99 (ms) ↓ | 0.156 |
| loaded_latency DSR p50 (ms) ↓ | 0.143 |
| loaded_latency DSR p95 (ms) ↓ | 0.179 |
| loaded_latency DSR p99 (ms) ↓ | 0.205 |
| tui_full DSR p50 (ms) ↓ | 0.361 |
| tui_full DSR p95 (ms) ↓ | 0.632 |
| tui_full DSR p99 (ms) ↓ | 0.702 |
| tui_full terminal CPU (%) ↓ | 10.8 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.184 |
| tui_partial DSR p95 (ms) ↓ | 0.237 |
| tui_partial DSR p99 (ms) ↓ | 0.259 |
| tui_partial terminal CPU (%) ↓ | 7.2 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 324.2 (324.2–324.2) |
| vtebench cursor_motion CPU ms/MiB ↓ | 4.633 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 104.8 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 321.4 (321.4–321.4) |
| vtebench dense_cells CPU ms/MiB ↓ | 4.630 |
| vtebench dense_cells physical footprint (MiB) ↓ | 104.9 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 302.3 (302.3–302.3) |
| vtebench light_cells CPU ms/MiB ↓ | 4.799 |
| vtebench light_cells physical footprint (MiB) ↓ | 104.9 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 241.2 (241.2–241.2) |
| vtebench medium_cells CPU ms/MiB ↓ | 5.631 |
| vtebench medium_cells physical footprint (MiB) ↓ | 105.1 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 28.8 (28.8–28.8) |
| vtebench scrolling CPU ms/MiB ↓ | 39.856 |
| vtebench scrolling physical footprint (MiB) ↓ | 140.5 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 43.8 (43.8–43.8) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 24.882 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 140.5 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 32.7 (32.7–32.7) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 32.948 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 140.6 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 153.9 (153.9–153.9) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 9.013 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 143.5 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 30.4 (30.4–30.4) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 35.425 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 143.5 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 39.2 (39.2–39.2) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 27.658 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 143.5 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 217.8 (217.8–217.8) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.055 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 141.7 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 282.3 (282.3–282.3) |
| vtebench unicode CPU ms/MiB ↓ | 7.558 |
| vtebench unicode physical footprint (MiB) ↓ | 187.4 |
| Peak sampled RSS (MiB) ↓ | 286.1 |
| Peak sampled physical footprint (MiB) ↓ | 189.0 |

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
- loomtty: `loomtty 0.2.1`; binary SHA-256 `160fd24d92bd28a646a27dcf6937c31dea57a51623ececdba505765446a528ac`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
