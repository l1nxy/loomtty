# macOS terminal benchmark

Generated: 2026-10-03T11:25:48.759174+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 1 | 1 |
| Launch → first DSR response (ms) ↓ | -233.7 (-233.7–-233.7) | -76733.4 (-76733.4–-76733.4) |
| Cold idle RSS (MiB) ↓ | 120.2 | 118.1 |
| Cold idle physical footprint (MiB) ↓ | 66.6 | 63.3 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.00 |
| Warm idle RSS (MiB) ↓ | 126.5 | 125.0 |
| Warm idle physical footprint (MiB) ↓ | 71.4 | 70.6 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 187.8 | 186.4 |
| After history physical footprint (MiB) ↓ | 132.5 | 132.7 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 240.2 (240.2–240.2) | 213.9 (213.9–213.9) |
| ascii terminal CPU (s) ↓ | 0.049 | 0.057 |
| truecolor parser throughput (MiB/s) ↑ | 282.5 (282.5–282.5) | 281.6 (281.6–281.6) |
| truecolor terminal CPU (s) ↓ | 0.044 | 0.044 |
| cjk parser throughput (MiB/s) ↑ | 291.1 (291.1–291.1) | 295.2 (295.2–295.2) |
| cjk terminal CPU (s) ↓ | 0.043 | 0.042 |
| emoji parser throughput (MiB/s) ↑ | 246.5 (246.5–246.5) | 300.6 (300.6–300.6) |
| emoji terminal CPU (s) ↓ | 0.052 | 0.042 |
| scroll parser throughput (MiB/s) ↑ | 196.4 (196.4–196.4) | 197.6 (197.6–197.6) |
| scroll terminal CPU (s) ↓ | 0.055 | 0.055 |
| cursor parser throughput (MiB/s) ↑ | 243.8 (243.8–243.8) | 228.7 (228.7–228.7) |
| cursor terminal CPU (s) ↓ | 0.047 | 0.050 |
| latency DSR p50 (ms) ↓ | 0.121 | 0.121 |
| latency DSR p95 (ms) ↓ | 0.227 | 0.213 |
| latency DSR p99 (ms) ↓ | 0.478 | 0.255 |
| loaded_latency DSR p50 (ms) ↓ | 0.156 | 0.155 |
| loaded_latency DSR p95 (ms) ↓ | 0.223 | 0.224 |
| loaded_latency DSR p99 (ms) ↓ | 0.266 | 0.261 |
| tui_full DSR p50 (ms) ↓ | 0.389 | 0.493 |
| tui_full DSR p95 (ms) ↓ | 0.628 | 1.053 |
| tui_full DSR p99 (ms) ↓ | 0.763 | 1.654 |
| tui_full terminal CPU (%) ↓ | 14.4 | 15.5 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.220 | 0.237 |
| tui_partial DSR p95 (ms) ↓ | 0.328 | 0.414 |
| tui_partial DSR p99 (ms) ↓ | 0.423 | 0.545 |
| tui_partial terminal CPU (%) ↓ | 11.0 | 10.6 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 187.9 | 186.4 |
| Peak sampled physical footprint (MiB) ↓ | 152.8 | 153.0 |

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

## Failed runs (not included in medians)

- after round 1: concurrent Rust build/test processes detected: 7808, 7810; retry when idle
