# macOS terminal benchmark

Generated: 2026-10-02T05:29:36.132054+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 1825.1 (1825.1–1825.1) |
| Cold idle RSS (MiB) ↓ | 116.8 |
| Cold idle physical footprint (MiB) ↓ | 60.8 |
| Cold idle CPU (% of one core) ↓ | 0.07 |
| Warm idle RSS (MiB) ↓ | 123.7 |
| Warm idle physical footprint (MiB) ↓ | 68.2 |
| Warm idle CPU (% of one core) ↓ | 0.01 |
| After history RSS (MiB) ↓ | 185.0 |
| After history physical footprint (MiB) ↓ | 129.5 |
| After history CPU (% of one core) ↓ | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 257.5 (257.5–257.5) |
| ascii terminal CPU (s) ↓ | 0.045 |
| truecolor parser throughput (MiB/s) ↑ | 311.3 (311.3–311.3) |
| truecolor terminal CPU (s) ↓ | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 306.9 (306.9–306.9) |
| cjk terminal CPU (s) ↓ | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 344.7 (344.7–344.7) |
| emoji terminal CPU (s) ↓ | 0.037 |
| scroll parser throughput (MiB/s) ↑ | 213.9 (213.9–213.9) |
| scroll terminal CPU (s) ↓ | 0.051 |
| cursor parser throughput (MiB/s) ↑ | 256.0 (256.0–256.0) |
| cursor terminal CPU (s) ↓ | 0.044 |
| latency DSR p50 (ms) ↓ | 0.089 |
| latency DSR p95 (ms) ↓ | 0.170 |
| latency DSR p99 (ms) ↓ | 0.361 |
| loaded_latency DSR p50 (ms) ↓ | 0.150 |
| loaded_latency DSR p95 (ms) ↓ | 0.198 |
| loaded_latency DSR p99 (ms) ↓ | 0.234 |
| tui_full DSR p50 (ms) ↓ | 0.385 |
| tui_full DSR p95 (ms) ↓ | 0.621 |
| tui_full DSR p99 (ms) ↓ | 0.714 |
| tui_full terminal CPU (%) ↓ | 13.0 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.177 |
| tui_partial DSR p95 (ms) ↓ | 0.241 |
| tui_partial DSR p99 (ms) ↓ | 0.278 |
| tui_partial terminal CPU (%) ↓ | 8.5 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 330.8 (330.8–330.8) |
| vtebench cursor_motion CPU ms/MiB ↓ | 4.564 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 101.9 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 321.4 (321.4–321.4) |
| vtebench dense_cells CPU ms/MiB ↓ | 4.654 |
| vtebench dense_cells physical footprint (MiB) ↓ | 102.2 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 302.3 (302.3–302.3) |
| vtebench light_cells CPU ms/MiB ↓ | 4.878 |
| vtebench light_cells physical footprint (MiB) ↓ | 102.1 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 235.9 (235.9–235.9) |
| vtebench medium_cells CPU ms/MiB ↓ | 5.723 |
| vtebench medium_cells physical footprint (MiB) ↓ | 112.5 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 28.2 (28.2–28.2) |
| vtebench scrolling CPU ms/MiB ↓ | 40.283 |
| vtebench scrolling physical footprint (MiB) ↓ | 120.2 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 43.8 (43.8–43.8) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 24.842 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 118.2 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 32.5 (32.5–32.5) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 33.098 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 120.2 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 144.2 (144.2–144.2) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 9.210 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 120.2 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 30.3 (30.3–30.3) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 35.492 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 120.3 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 38.9 (38.9–38.9) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 27.835 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 118.2 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 217.8 (217.8–217.8) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.127 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 120.4 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 287.3 (287.3–287.3) |
| vtebench unicode CPU ms/MiB ↓ | 7.404 |
| vtebench unicode physical footprint (MiB) ↓ | 163.3 |
| Peak sampled RSS (MiB) ↓ | 265.4 |
| Peak sampled physical footprint (MiB) ↓ | 163.3 |

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
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e8c11a399a3f46e2970ed519fb8128c48e0f9146e5f70068a4662ec18f0ca97c`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
