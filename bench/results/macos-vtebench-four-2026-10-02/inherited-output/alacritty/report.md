# macOS terminal benchmark

Generated: 2026-10-02T08:02:28.512958+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | alacritty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 432.2 (432.2–432.2) |
| Cold idle RSS (MiB) ↓ | 104.0 |
| Cold idle physical footprint (MiB) ↓ | 41.8 |
| Cold idle CPU (% of one core) ↓ | 0.04 |
| Warm idle RSS (MiB) ↓ | 110.8 |
| Warm idle physical footprint (MiB) ↓ | 46.6 |
| Warm idle CPU (% of one core) ↓ | 0.02 |
| After history RSS (MiB) ↓ | 137.8 |
| After history physical footprint (MiB) ↓ | 72.5 |
| After history CPU (% of one core) ↓ | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 178.0 (178.0–178.0) |
| ascii terminal CPU (s) ↓ | 0.046 |
| truecolor parser throughput (MiB/s) ↑ | 176.6 (176.6–176.6) |
| truecolor terminal CPU (s) ↓ | 0.049 |
| cjk parser throughput (MiB/s) ↑ | 154.2 (154.2–154.2) |
| cjk terminal CPU (s) ↓ | 0.055 |
| emoji parser throughput (MiB/s) ↑ | 189.5 (189.5–189.5) |
| emoji terminal CPU (s) ↓ | 0.045 |
| scroll parser throughput (MiB/s) ↑ | 173.6 (173.6–173.6) |
| scroll terminal CPU (s) ↓ | 0.043 |
| cursor parser throughput (MiB/s) ↑ | 198.0 (198.0–198.0) |
| cursor terminal CPU (s) ↓ | 0.042 |
| latency DSR p50 (ms) ↓ | 0.061 |
| latency DSR p95 (ms) ↓ | 4.525 |
| latency DSR p99 (ms) ↓ | 5.278 |
| loaded_latency DSR p50 (ms) ↓ | 0.194 |
| loaded_latency DSR p95 (ms) ↓ | 0.238 |
| loaded_latency DSR p99 (ms) ↓ | 3.682 |
| tui_full DSR p50 (ms) ↓ | 0.170 |
| tui_full DSR p95 (ms) ↓ | 1.071 |
| tui_full DSR p99 (ms) ↓ | 2.159 |
| tui_full terminal CPU (%) ↓ | 5.9 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.140 |
| tui_partial DSR p95 (ms) ↓ | 0.192 |
| tui_partial DSR p99 (ms) ↓ | 0.268 |
| tui_partial terminal CPU (%) ↓ | 6.6 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 274.8 (274.8–274.8) |
| vtebench cursor_motion CPU ms/MiB ↓ | 3.685 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 80.7 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 179.7 (179.7–179.7) |
| vtebench dense_cells CPU ms/MiB ↓ | 5.779 |
| vtebench dense_cells physical footprint (MiB) ↓ | 80.5 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 135.8 (135.8–135.8) |
| vtebench light_cells CPU ms/MiB ↓ | 7.790 |
| vtebench light_cells physical footprint (MiB) ↓ | 82.7 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 141.7 (141.7–141.7) |
| vtebench medium_cells CPU ms/MiB ↓ | 7.574 |
| vtebench medium_cells physical footprint (MiB) ↓ | 80.7 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 76.2 (76.2–76.2) |
| vtebench scrolling CPU ms/MiB ↓ | 13.411 |
| vtebench scrolling physical footprint (MiB) ↓ | 82.7 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 87.4 (87.4–87.4) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 11.529 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 80.7 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 53.1 (53.1–53.1) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 18.960 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 82.7 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 179.8 (179.8–179.8) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 5.970 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 82.8 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 47.3 (47.3–47.3) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 21.268 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 81.3 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 70.8 (70.8–70.8) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 14.260 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 81.3 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 119.4 (119.4–119.4) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 8.555 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 79.3 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 169.4 (169.4–169.4) |
| vtebench unicode CPU ms/MiB ↓ | 9.469 |
| vtebench unicode physical footprint (MiB) ↓ | 147.2 |
| Peak sampled RSS (MiB) ↓ | 236.8 |
| Peak sampled physical footprint (MiB) ↓ | 147.2 |

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
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
