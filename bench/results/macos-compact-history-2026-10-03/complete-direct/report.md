# macOS terminal benchmark

Generated: 2026-10-03T15:18:38.703684+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 1853.5 (774.5–3218.7) | 669.5 (184.7–2872.8) |
| Cold idle RSS (MiB) ↓ | 114.8 | 114.8 |
| Cold idle physical footprint (MiB) ↓ | 68.0 | 68.3 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| Warm idle RSS (MiB) ↓ | 121.9 | 121.5 |
| Warm idle physical footprint (MiB) ↓ | 70.9 | 70.1 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 183.5 | 170.5 |
| After history physical footprint (MiB) ↓ | 132.3 | 119.6 |
| After history CPU (% of one core) ↓ | 0.02 | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 249.6 (246.9–253.6) | 249.6 (247.1–249.9) |
| ascii terminal CPU (s) ↓ | 0.047 | 0.047 |
| truecolor parser throughput (MiB/s) ↑ | 302.2 (297.4–311.9) | 302.4 (294.6–303.0) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.042 |
| cjk parser throughput (MiB/s) ↑ | 314.5 (310.6–319.7) | 308.1 (303.6–310.5) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.041 |
| emoji parser throughput (MiB/s) ↑ | 320.5 (294.3–327.2) | 309.5 (265.4–335.3) |
| emoji terminal CPU (s) ↓ | 0.041 | 0.042 |
| scroll parser throughput (MiB/s) ↑ | 209.9 (206.8–211.4) | 209.3 (193.9–211.7) |
| scroll terminal CPU (s) ↓ | 0.053 | 0.052 |
| cursor parser throughput (MiB/s) ↑ | 251.0 (246.7–255.1) | 245.3 (236.8–251.5) |
| cursor terminal CPU (s) ↓ | 0.046 | 0.047 |
| latency DSR p50 (ms) ↓ | 0.168 | 0.164 |
| latency DSR p95 (ms) ↓ | 0.245 | 0.247 |
| latency DSR p99 (ms) ↓ | 0.343 | 0.359 |
| loaded_latency DSR p50 (ms) ↓ | 0.151 | 0.146 |
| loaded_latency DSR p95 (ms) ↓ | 0.190 | 0.191 |
| loaded_latency DSR p99 (ms) ↓ | 0.243 | 0.222 |
| tui_full DSR p50 (ms) ↓ | 0.437 | 0.456 |
| tui_full DSR p95 (ms) ↓ | 0.640 | 0.821 |
| tui_full DSR p99 (ms) ↓ | 0.738 | 1.255 |
| tui_full terminal CPU (%) ↓ | 14.8 | 15.0 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.228 | 0.223 |
| tui_partial DSR p95 (ms) ↓ | 0.498 | 0.305 |
| tui_partial DSR p99 (ms) ↓ | 0.807 | 0.402 |
| tui_partial terminal CPU (%) ↓ | 11.0 | 11.2 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.6 | 170.5 |
| Peak sampled physical footprint (MiB) ↓ | 152.6 | 139.8 |

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
