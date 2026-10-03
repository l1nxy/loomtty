# macOS terminal benchmark

Generated: 2026-10-03T11:32:13.804970+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 2252.8 (698.7–3260.6) | 734.1 (200.6–3366.6) |
| Cold idle RSS (MiB) ↓ | 116.5 | 116.3 |
| Cold idle physical footprint (MiB) ↓ | 61.5 | 62.3 |
| Cold idle CPU (% of one core) ↓ | 0.00 | 0.02 |
| Warm idle RSS (MiB) ↓ | 123.2 | 123.2 |
| Warm idle physical footprint (MiB) ↓ | 68.8 | 68.7 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 184.4 | 184.6 |
| After history physical footprint (MiB) ↓ | 129.8 | 129.9 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 238.2 (234.1–240.0) | 243.2 (236.8–245.4) |
| ascii terminal CPU (s) ↓ | 0.050 | 0.047 |
| truecolor parser throughput (MiB/s) ↑ | 284.6 (283.8–294.0) | 283.8 (277.9–285.6) |
| truecolor terminal CPU (s) ↓ | 0.044 | 0.044 |
| cjk parser throughput (MiB/s) ↑ | 289.5 (255.4–293.0) | 279.4 (248.4–289.3) |
| cjk terminal CPU (s) ↓ | 0.043 | 0.045 |
| emoji parser throughput (MiB/s) ↑ | 307.5 (285.5–314.0) | 302.3 (281.3–314.1) |
| emoji terminal CPU (s) ↓ | 0.041 | 0.043 |
| scroll parser throughput (MiB/s) ↑ | 196.7 (196.7–202.1) | 200.1 (197.0–202.4) |
| scroll terminal CPU (s) ↓ | 0.055 | 0.054 |
| cursor parser throughput (MiB/s) ↑ | 240.3 (227.2–240.4) | 237.5 (236.4–241.4) |
| cursor terminal CPU (s) ↓ | 0.048 | 0.049 |
| latency DSR p50 (ms) ↓ | 0.119 | 0.130 |
| latency DSR p95 (ms) ↓ | 0.234 | 0.248 |
| latency DSR p99 (ms) ↓ | 0.460 | 0.348 |
| loaded_latency DSR p50 (ms) ↓ | 0.147 | 0.148 |
| loaded_latency DSR p95 (ms) ↓ | 0.214 | 0.211 |
| loaded_latency DSR p99 (ms) ↓ | 0.262 | 0.243 |
| tui_full DSR p50 (ms) ↓ | 0.456 | 0.470 |
| tui_full DSR p95 (ms) ↓ | 0.680 | 0.916 |
| tui_full DSR p99 (ms) ↓ | 0.907 | 1.338 |
| tui_full terminal CPU (%) ↓ | 15.3 | 15.3 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.229 | 0.240 |
| tui_partial DSR p95 (ms) ↓ | 0.366 | 0.381 |
| tui_partial DSR p99 (ms) ↓ | 0.455 | 0.815 |
| tui_partial terminal CPU (%) ↓ | 10.8 | 10.6 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 184.5 | 184.7 |
| Peak sampled physical footprint (MiB) ↓ | 150.0 | 150.2 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `605218d1b067afd94b839c9f33f736583ca7669d230fab7d5f44b442efdfebb7`
  - loomtty-server SHA-256 `3262ba5dab25204a664268a7f1968d607d2427bdef5f60e0b6f0d9cf739d4581`
- after: `loomtty 0.2.1`; binary SHA-256 `e0be0cd392c71bdd7e6c4941c8b89e25ffe85975e9c1a2210b782f2f864846e2`
  - loomtty-server SHA-256 `3262ba5dab25204a664268a7f1968d607d2427bdef5f60e0b6f0d9cf739d4581`
