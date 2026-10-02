# macOS terminal benchmark

Generated: 2026-10-02T06:15:37.360923+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 663.2 (663.2–663.2) |
| Cold idle RSS (MiB) ↓ | 117.1 |
| Cold idle physical footprint (MiB) ↓ | 61.7 |
| Cold idle CPU (% of one core) ↓ | 0.00 |
| Warm idle RSS (MiB) ↓ | 124.1 |
| Warm idle physical footprint (MiB) ↓ | 68.2 |
| Warm idle CPU (% of one core) ↓ | 0.01 |
| After history RSS (MiB) ↓ | 185.5 |
| After history physical footprint (MiB) ↓ | 129.9 |
| After history CPU (% of one core) ↓ | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 244.6 (244.6–244.6) |
| ascii terminal CPU (s) ↓ | 0.047 |
| truecolor parser throughput (MiB/s) ↑ | 304.8 (304.8–304.8) |
| truecolor terminal CPU (s) ↓ | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 312.4 (312.4–312.4) |
| cjk terminal CPU (s) ↓ | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 342.5 (342.5–342.5) |
| emoji terminal CPU (s) ↓ | 0.038 |
| scroll parser throughput (MiB/s) ↑ | 213.3 (213.3–213.3) |
| scroll terminal CPU (s) ↓ | 0.050 |
| cursor parser throughput (MiB/s) ↑ | 254.6 (254.6–254.6) |
| cursor terminal CPU (s) ↓ | 0.045 |
| latency DSR p50 (ms) ↓ | 0.090 |
| latency DSR p95 (ms) ↓ | 0.156 |
| latency DSR p99 (ms) ↓ | 0.230 |
| loaded_latency DSR p50 (ms) ↓ | 0.141 |
| loaded_latency DSR p95 (ms) ↓ | 0.193 |
| loaded_latency DSR p99 (ms) ↓ | 0.226 |
| tui_full DSR p50 (ms) ↓ | 0.343 |
| tui_full DSR p95 (ms) ↓ | 0.634 |
| tui_full DSR p99 (ms) ↓ | 0.719 |
| tui_full terminal CPU (%) ↓ | 11.9 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.206 |
| tui_partial DSR p95 (ms) ↓ | 0.387 |
| tui_partial DSR p99 (ms) ↓ | 0.618 |
| tui_partial terminal CPU (%) ↓ | 7.9 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 234.9 (234.9–234.9) |
| vtebench cursor_motion CPU ms/MiB ↓ | 6.528 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 102.5 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 269.5 (269.5–269.5) |
| vtebench dense_cells CPU ms/MiB ↓ | 5.752 |
| vtebench dense_cells physical footprint (MiB) ↓ | 102.8 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 296.7 (296.7–296.7) |
| vtebench light_cells CPU ms/MiB ↓ | 4.944 |
| vtebench light_cells physical footprint (MiB) ↓ | 102.8 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 230.8 (230.8–230.8) |
| vtebench medium_cells CPU ms/MiB ↓ | 6.370 |
| vtebench medium_cells physical footprint (MiB) ↓ | 111.2 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 82.1 (82.1–82.1) |
| vtebench scrolling CPU ms/MiB ↓ | 16.704 |
| vtebench scrolling physical footprint (MiB) ↓ | 120.1 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 95.2 (95.2–95.2) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 14.657 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 120.1 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 56.3 (56.3–56.3) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 22.747 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 120.1 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 173.9 (173.9–173.9) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 8.145 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 120.7 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 49.4 (49.4–49.4) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 25.456 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 118.7 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 75.5 (75.5–75.5) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 18.331 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 120.7 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 214.9 (214.9–214.9) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.632 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 128.8 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 300.8 (300.8–300.8) |
| vtebench unicode CPU ms/MiB ↓ | 6.983 |
| vtebench unicode physical footprint (MiB) ↓ | 163.4 |
| Peak sampled RSS (MiB) ↓ | 265.3 |
| Peak sampled physical footprint (MiB) ↓ | 163.4 |

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
- PTY output modes (mode, flags): [('original', 3)]. Missing legacy metadata means raw output. Payload byte counts precede kernel output processing.
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e8c11a399a3f46e2970ed519fb8128c48e0f9146e5f70068a4662ec18f0ca97c`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
