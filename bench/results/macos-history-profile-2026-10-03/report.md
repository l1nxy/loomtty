# macOS terminal benchmark

Generated: 2026-10-03T15:29:55.856884+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 1 | 1 |
| Launch → first DSR response (ms) ↓ | 1159.3 (1159.3–1159.3) | 619.6 (619.6–619.6) |
| Cold idle RSS (MiB) ↓ | 115.0 | 114.8 |
| Cold idle physical footprint (MiB) ↓ | 68.1 | 67.9 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.01 |
| Warm idle RSS (MiB) ↓ | 122.0 | 121.6 |
| Warm idle physical footprint (MiB) ↓ | 76.4 | 75.9 |
| Warm idle CPU (% of one core) ↓ | 0.02 | 0.01 |
| After history RSS (MiB) ↓ | 183.3 | 170.8 |
| After history physical footprint (MiB) ↓ | 134.2 | 121.7 |
| After history CPU (% of one core) ↓ | 0.02 | 0.03 |
| ascii parser throughput (MiB/s) ↑ | 202.8 (202.8–202.8) | 194.6 (194.6–194.6) |
| ascii terminal CPU (s) ↓ | 0.004 | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 221.4 (221.4–221.4) | 223.3 (223.3–223.3) |
| truecolor terminal CPU (s) ↓ | 0.003 | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 212.9 (212.9–212.9) | 230.4 (230.4–230.4) |
| cjk terminal CPU (s) ↓ | 0.003 | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 223.0 (223.0–223.0) | 219.0 (219.0–219.0) |
| emoji terminal CPU (s) ↓ | 0.003 | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 180.0 (180.0–180.0) | 175.6 (175.6–175.6) |
| scroll terminal CPU (s) ↓ | 0.003 | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 200.5 (200.5–200.5) | 204.0 (204.0–204.0) |
| cursor terminal CPU (s) ↓ | 0.003 | 0.003 |
| latency DSR p50 (ms) ↓ | 0.046 | 0.046 |
| latency DSR p95 (ms) ↓ | 0.059 | 0.102 |
| latency DSR p99 (ms) ↓ | 0.075 | 0.152 |
| loaded_latency DSR p50 (ms) ↓ | 0.158 | 0.164 |
| loaded_latency DSR p95 (ms) ↓ | 0.198 | 0.195 |
| loaded_latency DSR p99 (ms) ↓ | 0.201 | 0.197 |
| tui_full DSR p50 (ms) ↓ | 0.402 | 0.420 |
| tui_full DSR p95 (ms) ↓ | 0.601 | 0.747 |
| tui_full DSR p99 (ms) ↓ | 0.675 | 1.015 |
| tui_full terminal CPU (%) ↓ | 13.7 | 14.0 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.206 | 0.207 |
| tui_partial DSR p95 (ms) ↓ | 0.270 | 0.275 |
| tui_partial DSR p99 (ms) ↓ | 0.340 | 0.413 |
| tui_partial terminal CPU (%) ↓ | 9.0 | 9.3 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.3 | 170.8 |
| Peak sampled physical footprint (MiB) ↓ | 152.2 | 139.7 |

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
- after: `loomtty 0.2.1`; binary SHA-256 `eefe9a96a85fc8f454d67fb866305fbbbe00e2b7bf8ac9458ad8e2176e008f1e`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
