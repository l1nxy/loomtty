# macOS terminal benchmark

Generated: 2026-10-02T08:07:14.200277+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | ghostty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 614.4 (614.4–614.4) |
| Cold idle RSS (MiB) ↓ | 120.8 |
| Cold idle physical footprint (MiB) ↓ | 67.0 |
| Cold idle CPU (% of one core) ↓ | 0.77 |
| Warm idle RSS (MiB) ↓ | 129.4 |
| Warm idle physical footprint (MiB) ↓ | 85.5 |
| Warm idle CPU (% of one core) ↓ | 0.70 |
| After history RSS (MiB) ↓ | 138.2 |
| After history physical footprint (MiB) ↓ | 92.2 |
| After history CPU (% of one core) ↓ | 0.72 |
| ascii parser throughput (MiB/s) ↑ | 123.6 (123.6–123.6) |
| ascii terminal CPU (s) ↓ | 0.103 |
| truecolor parser throughput (MiB/s) ↑ | 121.8 (121.8–121.8) |
| truecolor terminal CPU (s) ↓ | 0.102 |
| cjk parser throughput (MiB/s) ↑ | 139.8 (139.8–139.8) |
| cjk terminal CPU (s) ↓ | 0.098 |
| emoji parser throughput (MiB/s) ↑ | 150.8 (150.8–150.8) |
| emoji terminal CPU (s) ↓ | 0.096 |
| scroll parser throughput (MiB/s) ↑ | 96.3 (96.3–96.3) |
| scroll terminal CPU (s) ↓ | 0.103 |
| cursor parser throughput (MiB/s) ↑ | 123.8 (123.8–123.8) |
| cursor terminal CPU (s) ↓ | 0.105 |
| latency DSR p50 (ms) ↓ | 0.101 |
| latency DSR p95 (ms) ↓ | 0.133 |
| latency DSR p99 (ms) ↓ | 0.209 |
| loaded_latency DSR p50 (ms) ↓ | 0.264 |
| loaded_latency DSR p95 (ms) ↓ | 0.317 |
| loaded_latency DSR p99 (ms) ↓ | 0.402 |
| tui_full DSR p50 (ms) ↓ | 0.356 |
| tui_full DSR p95 (ms) ↓ | 0.573 |
| tui_full DSR p99 (ms) ↓ | 0.686 |
| tui_full terminal CPU (%) ↓ | 13.3 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.202 |
| tui_partial DSR p95 (ms) ↓ | 0.252 |
| tui_partial DSR p99 (ms) ↓ | 0.316 |
| tui_partial terminal CPU (%) ↓ | 7.0 |
| tui_partial parser deadline misses ↓ | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 115.8 (115.8–115.8) |
| vtebench cursor_motion CPU ms/MiB ↓ | 15.319 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 85.4 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 110.7 (110.7–110.7) |
| vtebench dense_cells CPU ms/MiB ↓ | 12.442 |
| vtebench dense_cells physical footprint (MiB) ↓ | 86.5 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 126.2 (126.2–126.2) |
| vtebench light_cells CPU ms/MiB ↓ | 12.378 |
| vtebench light_cells physical footprint (MiB) ↓ | 86.5 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 102.3 (102.3–102.3) |
| vtebench medium_cells CPU ms/MiB ↓ | 15.644 |
| vtebench medium_cells physical footprint (MiB) ↓ | 87.4 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 59.0 (59.0–59.0) |
| vtebench scrolling CPU ms/MiB ↓ | 26.735 |
| vtebench scrolling physical footprint (MiB) ↓ | 87.6 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 48.6 (48.6–48.6) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 31.311 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 87.6 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 53.2 (53.2–53.2) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 28.933 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 87.6 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 127.0 (127.0–127.0) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 12.381 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 87.6 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 60.3 (60.3–60.3) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 21.046 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 87.6 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 61.3 (61.3–61.3) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 21.552 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 87.6 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 97.7 (97.7–97.7) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 14.125 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 87.8 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 138.7 (138.7–138.7) |
| vtebench unicode CPU ms/MiB ↓ | 12.378 |
| vtebench unicode physical footprint (MiB) ↓ | 216.3 |
| Peak sampled RSS (MiB) ↓ | 318.5 |
| Peak sampled physical footprint (MiB) ↓ | 216.5 |

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
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`
