# macOS terminal benchmark

Generated: 2026-09-29T17:05:31.758935+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 846.3 (826.7–851.2) | 844.3 (833.3–961.6) |
| Cold idle RSS (MiB) ↓ | 101.0 | 100.9 |
| Cold idle physical footprint (MiB) ↓ | 56.5 | 56.3 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.03 |
| Warm idle RSS (MiB) ↓ | 108.0 | 107.7 |
| Warm idle physical footprint (MiB) ↓ | 57.1 | 57.1 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 155.9 | 153.6 |
| After history physical footprint (MiB) ↓ | 100.9 | 100.9 |
| After history CPU (% of one core) ↓ | 0.02 | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 156.8 (153.7–156.8) | 218.6 (215.5–218.9) |
| ascii terminal CPU (s) ↓ | 0.067 | 0.051 |
| truecolor parser throughput (MiB/s) ↑ | 164.2 (160.8–167.9) | 263.4 (262.2–265.4) |
| truecolor terminal CPU (s) ↓ | 0.063 | 0.044 |
| cjk parser throughput (MiB/s) ↑ | 178.9 (177.1–179.4) | 270.6 (266.8–271.5) |
| cjk terminal CPU (s) ↓ | 0.059 | 0.043 |
| emoji parser throughput (MiB/s) ↑ | 195.0 (193.3–195.9) | 299.0 (286.8–306.6) |
| emoji terminal CPU (s) ↓ | 0.055 | 0.041 |
| scroll parser throughput (MiB/s) ↑ | 140.1 (139.5–140.1) | 183.7 (183.0–190.4) |
| scroll terminal CPU (s) ↓ | 0.072 | 0.057 |
| cursor parser throughput (MiB/s) ↑ | 148.1 (147.8–149.8) | 216.0 (216.0–218.2) |
| cursor terminal CPU (s) ↓ | 0.069 | 0.051 |
| latency DSR p50 (ms) ↓ | 11.618 | 0.101 |
| latency DSR p95 (ms) ↓ | 13.629 | 0.137 |
| latency DSR p99 (ms) ↓ | 14.093 | 0.159 |
| loaded_latency DSR p50 (ms) ↓ | 0.274 | 0.154 |
| loaded_latency DSR p95 (ms) ↓ | 0.354 | 0.189 |
| loaded_latency DSR p99 (ms) ↓ | 19.201 | 0.224 |
| tui_full DSR p50 (ms) ↓ | 7.288 | 0.497 |
| tui_full DSR p95 (ms) ↓ | 17.582 | 0.746 |
| tui_full DSR p99 (ms) ↓ | 18.963 | 0.824 |
| tui_full terminal CPU (%) ↓ | 16.2 | 14.2 |
| tui_full parser deadline misses ↓ | 42 | 0 |
| tui_partial DSR p50 (ms) ↓ | 7.754 | 0.141 |
| tui_partial DSR p95 (ms) ↓ | 17.337 | 0.207 |
| tui_partial DSR p99 (ms) ↓ | 18.727 | 0.241 |
| tui_partial terminal CPU (%) ↓ | 5.1 | 6.5 |
| tui_partial parser deadline misses ↓ | 38 | 0 |
| Peak sampled RSS (MiB) ↓ | 156.0 | 153.6 |
| Peak sampled physical footprint (MiB) ↓ | 121.2 | 121.2 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.1.0`; binary SHA-256 `c5e7470c783b76b8f4cdca8300d1429424a54cfc6ba30be826310281fbed154f`
  - loomtty-server SHA-256 `954e5d06dd87f95a114d36ec6d2cb2eb525ad0c123c9e6d66fbaecaf725a2229`
- after: `loomtty 0.1.0`; binary SHA-256 `c5e7470c783b76b8f4cdca8300d1429424a54cfc6ba30be826310281fbed154f`
  - loomtty-server SHA-256 `74cc781612c2c61e134d94fca24b60124affed7d168adfc19b8c740f1c7d8737`

## Failed runs (not included in medians)

- before round 1: concurrent Rust build/test processes detected: 64727; retry when idle
