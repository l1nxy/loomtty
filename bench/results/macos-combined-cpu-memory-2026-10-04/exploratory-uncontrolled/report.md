# macOS terminal benchmark

Generated: 2026-10-03T16:01:24.372770+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 611.5 (488.4–1690.0) | 626.2 (534.7–857.7) |
| Cold idle RSS (MiB) ↓ | 115.2 | 114.8 |
| Cold idle physical footprint (MiB) ↓ | 68.7 | 68.3 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.03 |
| Warm idle RSS (MiB) ↓ | 122.4 | 121.8 |
| Warm idle physical footprint (MiB) ↓ | 70.9 | 70.3 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.02 |
| After history RSS (MiB) ↓ | 183.7 | 171.9 |
| After history physical footprint (MiB) ↓ | 132.8 | 120.3 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 252.8 (158.5–253.6) | 253.0 (226.8–255.0) |
| ascii terminal CPU (s) ↓ | 0.047 | 0.047 |
| truecolor parser throughput (MiB/s) ↑ | 306.5 (152.7–315.5) | 280.9 (260.7–295.1) |
| truecolor terminal CPU (s) ↓ | 0.042 | 0.045 |
| cjk parser throughput (MiB/s) ↑ | 311.4 (133.9–318.3) | 313.6 (239.7–323.1) |
| cjk terminal CPU (s) ↓ | 0.043 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 330.4 (146.9–333.3) | 327.1 (238.1–337.9) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.040 |
| scroll parser throughput (MiB/s) ↑ | 184.2 (121.9–209.8) | 209.7 (198.6–213.1) |
| scroll terminal CPU (s) ↓ | 0.069 | 0.052 |
| cursor parser throughput (MiB/s) ↑ | 246.7 (104.0–251.2) | 242.5 (231.6–255.2) |
| cursor terminal CPU (s) ↓ | 0.047 | 0.052 |
| latency DSR p50 (ms) ↓ | 0.157 | 0.140 |
| latency DSR p95 (ms) ↓ | 0.232 | 0.169 |
| latency DSR p99 (ms) ↓ | 0.332 | 0.206 |
| loaded_latency DSR p50 (ms) ↓ | 0.275 | 0.172 |
| loaded_latency DSR p95 (ms) ↓ | 0.355 | 0.220 |
| loaded_latency DSR p99 (ms) ↓ | 0.448 | 0.262 |
| tui_full DSR p50 (ms) ↓ | 0.417 | 0.330 |
| tui_full DSR p95 (ms) ↓ | 0.639 | 0.498 |
| tui_full DSR p99 (ms) ↓ | 0.829 | 0.588 |
| tui_full terminal CPU (%) ↓ | 14.6 | 11.1 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.176 | 0.173 |
| tui_partial DSR p95 (ms) ↓ | 0.256 | 0.217 |
| tui_partial DSR p99 (ms) ↓ | 0.317 | 0.281 |
| tui_partial terminal CPU (%) ↓ | 9.2 | 8.7 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.7 | 172.0 |
| Peak sampled physical footprint (MiB) ↓ | 153.1 | 140.6 |

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
