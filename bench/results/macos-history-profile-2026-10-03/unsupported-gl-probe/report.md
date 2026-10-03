# macOS terminal benchmark

Generated: 2026-10-03T15:34:30.193277+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 1 | — |
| Launch → first DSR response (ms) ↓ | 1074.5 (1074.5–1074.5) | — |
| Cold idle RSS (MiB) ↓ | 114.6 | — |
| Cold idle physical footprint (MiB) ↓ | 67.8 | — |
| Cold idle CPU (% of one core) ↓ | 0.04 | — |
| Warm idle RSS (MiB) ↓ | 121.9 | — |
| Warm idle physical footprint (MiB) ↓ | 70.0 | — |
| Warm idle CPU (% of one core) ↓ | 0.05 | — |
| After history RSS (MiB) ↓ | 183.4 | — |
| After history physical footprint (MiB) ↓ | 132.2 | — |
| After history CPU (% of one core) ↓ | 0.01 | — |
| ascii parser throughput (MiB/s) ↑ | 254.2 (254.2–254.2) | — |
| ascii terminal CPU (s) ↓ | 0.045 | — |
| truecolor parser throughput (MiB/s) ↑ | 304.1 (304.1–304.1) | — |
| truecolor terminal CPU (s) ↓ | 0.041 | — |
| cjk parser throughput (MiB/s) ↑ | 306.5 (306.5–306.5) | — |
| cjk terminal CPU (s) ↓ | 0.041 | — |
| emoji parser throughput (MiB/s) ↑ | 345.0 (345.0–345.0) | — |
| emoji terminal CPU (s) ↓ | 0.038 | — |
| scroll parser throughput (MiB/s) ↑ | 217.2 (217.2–217.2) | — |
| scroll terminal CPU (s) ↓ | 0.050 | — |
| cursor parser throughput (MiB/s) ↑ | 252.6 (252.6–252.6) | — |
| cursor terminal CPU (s) ↓ | 0.045 | — |
| latency DSR p50 (ms) ↓ | 0.100 | — |
| latency DSR p95 (ms) ↓ | 0.139 | — |
| latency DSR p99 (ms) ↓ | 0.192 | — |
| loaded_latency DSR p50 (ms) ↓ | 0.143 | — |
| loaded_latency DSR p95 (ms) ↓ | 0.187 | — |
| loaded_latency DSR p99 (ms) ↓ | 0.211 | — |
| tui_full DSR p50 (ms) ↓ | 0.403 | — |
| tui_full DSR p95 (ms) ↓ | 0.603 | — |
| tui_full DSR p99 (ms) ↓ | 0.679 | — |
| tui_full terminal CPU (%) ↓ | 14.7 | — |
| tui_full parser deadline misses ↓ | 0 | — |
| tui_partial DSR p50 (ms) ↓ | 0.208 | — |
| tui_partial DSR p95 (ms) ↓ | 0.277 | — |
| tui_partial DSR p99 (ms) ↓ | 0.447 | — |
| tui_partial terminal CPU (%) ↓ | 9.5 | — |
| tui_partial parser deadline misses ↓ | 0 | — |
| Peak sampled RSS (MiB) ↓ | 183.4 | — |
| Peak sampled physical footprint (MiB) ↓ | 152.4 | — |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `e467ffb2a2659a1a347461ae969cf5270393bb5091b2e1e55da7e1e59afc52af`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- after: `loomtty 0.2.1`; binary SHA-256 `e467ffb2a2659a1a347461ae969cf5270393bb5091b2e1e55da7e1e59afc52af`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`

## Failed runs (not included in medians)

- after round 0: timed out waiting for /private/tmp/loom-backend-probe-1003/r0-after/ready.json
