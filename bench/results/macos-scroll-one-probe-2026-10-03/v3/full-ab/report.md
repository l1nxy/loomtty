# macOS terminal benchmark

Generated: 2026-10-03T12:47:40.944326+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 678.3 (631.8–1009.6) | 787.1 (691.5–1173.2) |
| Cold idle RSS (MiB) ↓ | 114.8 | 115.0 |
| Cold idle physical footprint (MiB) ↓ | 68.1 | 68.2 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.03 |
| Warm idle RSS (MiB) ↓ | 122.0 | 122.0 |
| Warm idle physical footprint (MiB) ↓ | 70.2 | 70.4 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 183.5 | 183.4 |
| After history physical footprint (MiB) ↓ | 132.4 | 132.3 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 250.2 (240.6–251.1) | 254.4 (245.5–256.9) |
| ascii terminal CPU (s) ↓ | 0.047 | 0.046 |
| truecolor parser throughput (MiB/s) ↑ | 299.5 (297.7–303.6) | 305.4 (298.1–305.9) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.041 |
| cjk parser throughput (MiB/s) ↑ | 307.3 (299.6–322.3) | 311.4 (297.8–322.1) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 339.6 (334.4–341.3) | 336.2 (335.7–354.8) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 211.0 (208.2–215.2) | 231.6 (229.0–231.9) |
| scroll terminal CPU (s) ↓ | 0.051 | 0.048 |
| cursor parser throughput (MiB/s) ↑ | 251.5 (240.2–253.0) | 246.0 (243.6–248.6) |
| cursor terminal CPU (s) ↓ | 0.045 | 0.047 |
| latency DSR p50 (ms) ↓ | 0.123 | 0.122 |
| latency DSR p95 (ms) ↓ | 0.187 | 0.193 |
| latency DSR p99 (ms) ↓ | 0.250 | 0.234 |
| loaded_latency DSR p50 (ms) ↓ | 0.144 | 0.143 |
| loaded_latency DSR p95 (ms) ↓ | 0.184 | 0.190 |
| loaded_latency DSR p99 (ms) ↓ | 0.214 | 0.227 |
| tui_full DSR p50 (ms) ↓ | 0.391 | 0.415 |
| tui_full DSR p95 (ms) ↓ | 0.592 | 0.689 |
| tui_full DSR p99 (ms) ↓ | 0.714 | 0.880 |
| tui_full terminal CPU (%) ↓ | 12.4 | 12.6 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.224 | 0.203 |
| tui_partial DSR p95 (ms) ↓ | 0.390 | 0.293 |
| tui_partial DSR p99 (ms) ↓ | 0.573 | 0.367 |
| tui_partial terminal CPU (%) ↓ | 8.7 | 8.7 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.6 | 183.5 |
| Peak sampled physical footprint (MiB) ↓ | 152.7 | 152.6 |

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
  - loomtty-server SHA-256 `cfba8d029b164b476f6caefe2d17fc5c212a764e10844898f1a4a990ef835f78`
