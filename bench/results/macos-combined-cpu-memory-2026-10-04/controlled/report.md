# macOS terminal benchmark

Generated: 2026-10-03T16:27:23.304023+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 987.7 (579.4–1038.2) | 658.8 (263.7–832.1) |
| Cold idle RSS (MiB) ↓ | 116.1 | 116.6 |
| Cold idle physical footprint (MiB) ↓ | 61.1 | 61.5 |
| Cold idle CPU (% of one core) ↓ | 0.01 | 0.03 |
| Warm idle RSS (MiB) ↓ | 123.0 | 123.6 |
| Warm idle physical footprint (MiB) ↓ | 68.4 | 68.9 |
| Warm idle CPU (% of one core) ↓ | 0.03 | 0.05 |
| After history RSS (MiB) ↓ | 184.4 | 172.2 |
| After history physical footprint (MiB) ↓ | 130.4 | 117.5 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 236.0 (132.5–244.8) | 243.9 (242.3–244.7) |
| ascii terminal CPU (s) ↓ | 0.055 | 0.049 |
| truecolor parser throughput (MiB/s) ↑ | 243.0 (153.1–290.4) | 286.9 (280.3–300.3) |
| truecolor terminal CPU (s) ↓ | 0.055 | 0.043 |
| cjk parser throughput (MiB/s) ↑ | 216.3 (129.0–308.1) | 300.4 (295.4–308.6) |
| cjk terminal CPU (s) ↓ | 0.063 | 0.042 |
| emoji parser throughput (MiB/s) ↑ | 305.4 (149.1–322.2) | 313.1 (292.1–329.8) |
| emoji terminal CPU (s) ↓ | 0.042 | 0.042 |
| scroll parser throughput (MiB/s) ↑ | 201.5 (98.3–202.8) | 205.8 (201.5–206.5) |
| scroll terminal CPU (s) ↓ | 0.055 | 0.054 |
| cursor parser throughput (MiB/s) ↑ | 234.6 (103.5–247.5) | 242.4 (241.0–246.2) |
| cursor terminal CPU (s) ↓ | 0.050 | 0.048 |
| latency DSR p50 (ms) ↓ | 0.189 | 0.200 |
| latency DSR p95 (ms) ↓ | 0.313 | 0.320 |
| latency DSR p99 (ms) ↓ | 0.481 | 0.531 |
| loaded_latency DSR p50 (ms) ↓ | 0.148 | 0.150 |
| loaded_latency DSR p95 (ms) ↓ | 0.213 | 0.213 |
| loaded_latency DSR p99 (ms) ↓ | 0.244 | 0.247 |
| tui_full DSR p50 (ms) ↓ | 0.363 | 0.477 |
| tui_full DSR p95 (ms) ↓ | 0.536 | 0.692 |
| tui_full DSR p99 (ms) ↓ | 0.604 | 0.831 |
| tui_full terminal CPU (%) ↓ | 10.2 | 14.4 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.102 | 0.243 |
| tui_partial DSR p95 (ms) ↓ | 0.281 | 0.395 |
| tui_partial DSR p99 (ms) ↓ | 0.430 | 0.596 |
| tui_partial terminal CPU (%) ↓ | 7.7 | 10.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 184.4 | 172.2 |
| Peak sampled physical footprint (MiB) ↓ | 150.6 | 137.8 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `3a0479115c3e0990c6765a61dd02586e27c3401ac324c43059be93c0b6c7ae94`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- after: `loomtty 0.2.1`; binary SHA-256 `3d186eb428226296f8c8df0223adfe21076722be3912a3e04b335f30266a030b`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
