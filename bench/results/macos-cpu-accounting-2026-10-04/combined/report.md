# macOS terminal benchmark

Generated: 2026-10-03T16:41:19.367388+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 657.2 (650.8–1177.9) | 1638.5 (570.1–2271.4) |
| Cold idle RSS (MiB) ↓ | 116.1 | 116.3 |
| Cold idle physical footprint (MiB) ↓ | 61.2 | 61.2 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.02 |
| Warm idle RSS (MiB) ↓ | 123.2 | 123.2 |
| Warm idle physical footprint (MiB) ↓ | 68.3 | 68.5 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 184.8 | 172.0 |
| After history physical footprint (MiB) ↓ | 130.5 | 118.1 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 125.8 (102.0–245.9) | 234.5 (193.6–246.6) |
| ascii terminal CPU (s) ↓ | 0.109 | 0.056 |
| truecolor parser throughput (MiB/s) ↑ | 246.0 (139.3–279.4) | 244.9 (235.6–298.8) |
| truecolor terminal CPU (s) ↓ | 0.054 | 0.055 |
| cjk parser throughput (MiB/s) ↑ | 215.6 (133.7–297.1) | 241.2 (239.5–309.0) |
| cjk terminal CPU (s) ↓ | 0.062 | 0.055 |
| emoji parser throughput (MiB/s) ↑ | 153.3 (152.5–303.5) | 239.5 (218.0–322.6) |
| emoji terminal CPU (s) ↓ | 0.088 | 0.057 |
| scroll parser throughput (MiB/s) ↑ | 157.7 (105.6–199.9) | 206.4 (190.0–208.9) |
| scroll terminal CPU (s) ↓ | 0.074 | 0.053 |
| cursor parser throughput (MiB/s) ↑ | 193.7 (161.6–247.4) | 242.9 (207.8–245.5) |
| cursor terminal CPU (s) ↓ | 0.065 | 0.048 |
| latency DSR p50 (ms) ↓ | 0.090 | 0.184 |
| latency DSR p95 (ms) ↓ | 0.175 | 0.276 |
| latency DSR p99 (ms) ↓ | 0.204 | 0.326 |
| loaded_latency DSR p50 (ms) ↓ | 0.150 | 0.167 |
| loaded_latency DSR p95 (ms) ↓ | 0.202 | 0.245 |
| loaded_latency DSR p99 (ms) ↓ | 0.225 | 0.476 |
| tui_full DSR p50 (ms) ↓ | 0.368 | 0.403 |
| tui_full DSR p95 (ms) ↓ | 0.797 | 0.670 |
| tui_full DSR p99 (ms) ↓ | 1.361 | 0.827 |
| tui_full terminal CPU (%) ↓ | 11.7 | 13.5 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.206 | 0.236 |
| tui_partial DSR p95 (ms) ↓ | 0.355 | 0.325 |
| tui_partial DSR p99 (ms) ↓ | 0.473 | 0.466 |
| tui_partial terminal CPU (%) ↓ | 10.0 | 9.9 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 184.9 | 172.0 |
| Peak sampled physical footprint (MiB) ↓ | 150.8 | 138.3 |

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
