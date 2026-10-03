# macOS terminal benchmark

Generated: 2026-10-03T15:02:22.418987+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 672.3 (602.9–1241.8) | 867.2 (607.9–1732.0) |
| Cold idle RSS (MiB) ↓ | 114.6 | 115.0 |
| Cold idle physical footprint (MiB) ↓ | 67.8 | 68.1 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.03 |
| Warm idle RSS (MiB) ↓ | 121.7 | 122.1 |
| Warm idle physical footprint (MiB) ↓ | 69.9 | 70.2 |
| Warm idle CPU (% of one core) ↓ | 0.02 | 0.01 |
| After history RSS (MiB) ↓ | 183.2 | 170.9 |
| After history physical footprint (MiB) ↓ | 132.2 | 119.5 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 252.0 (242.9–254.1) | 239.6 (220.2–258.0) |
| ascii terminal CPU (s) ↓ | 0.046 | 0.051 |
| truecolor parser throughput (MiB/s) ↑ | 304.4 (302.9–309.3) | 305.9 (303.6–307.1) |
| truecolor terminal CPU (s) ↓ | 0.040 | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 294.5 (251.9–313.3) | 315.0 (131.4–324.4) |
| cjk terminal CPU (s) ↓ | 0.042 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 329.7 (242.9–345.6) | 333.7 (253.1–345.6) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 208.8 (207.0–209.5) | 207.7 (200.8–212.3) |
| scroll terminal CPU (s) ↓ | 0.052 | 0.052 |
| cursor parser throughput (MiB/s) ↑ | 252.9 (252.3–255.1) | 254.5 (202.3–254.7) |
| cursor terminal CPU (s) ↓ | 0.045 | 0.045 |
| latency DSR p50 (ms) ↓ | 0.110 | 0.110 |
| latency DSR p95 (ms) ↓ | 0.176 | 0.164 |
| latency DSR p99 (ms) ↓ | 0.241 | 0.271 |
| loaded_latency DSR p50 (ms) ↓ | 0.144 | 0.142 |
| loaded_latency DSR p95 (ms) ↓ | 0.191 | 0.185 |
| loaded_latency DSR p99 (ms) ↓ | 0.222 | 0.203 |
| tui_full DSR p50 (ms) ↓ | 0.404 | 0.369 |
| tui_full DSR p95 (ms) ↓ | 0.610 | 0.561 |
| tui_full DSR p99 (ms) ↓ | 0.753 | 0.661 |
| tui_full terminal CPU (%) ↓ | 14.0 | 13.5 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.208 | 0.181 |
| tui_partial DSR p95 (ms) ↓ | 0.271 | 0.264 |
| tui_partial DSR p99 (ms) ↓ | 0.404 | 0.348 |
| tui_partial terminal CPU (%) ↓ | 9.3 | 8.6 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.2 | 170.9 |
| Peak sampled physical footprint (MiB) ↓ | 152.4 | 139.8 |

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
- after: `loomtty 0.2.1`; binary SHA-256 `dc7669639f6444bd423610fca963097a9365aff02399ef7338af6474c83e9a8f`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
