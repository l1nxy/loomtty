# macOS terminal benchmark

Generated: 2026-10-03T14:49:13.342574+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 1 | — |
| Launch → first DSR response (ms) ↓ | 630.2 (630.2–630.2) | — |
| Cold idle RSS (MiB) ↓ | 114.6 | — |
| Cold idle physical footprint (MiB) ↓ | 67.8 | — |
| Cold idle CPU (% of one core) ↓ | 0.04 | — |
| Warm idle RSS (MiB) ↓ | 121.8 | — |
| Warm idle physical footprint (MiB) ↓ | 69.9 | — |
| Warm idle CPU (% of one core) ↓ | 0.02 | — |
| After history RSS (MiB) ↓ | 183.2 | — |
| After history physical footprint (MiB) ↓ | 132.0 | — |
| After history CPU (% of one core) ↓ | 0.01 | — |
| ascii parser throughput (MiB/s) ↑ | 254.0 (254.0–254.0) | — |
| ascii terminal CPU (s) ↓ | 0.046 | — |
| truecolor parser throughput (MiB/s) ↑ | 305.0 (305.0–305.0) | — |
| truecolor terminal CPU (s) ↓ | 0.040 | — |
| cjk parser throughput (MiB/s) ↑ | 315.7 (315.7–315.7) | — |
| cjk terminal CPU (s) ↓ | 0.039 | — |
| emoji parser throughput (MiB/s) ↑ | 325.7 (325.7–325.7) | — |
| emoji terminal CPU (s) ↓ | 0.040 | — |
| scroll parser throughput (MiB/s) ↑ | 210.2 (210.2–210.2) | — |
| scroll terminal CPU (s) ↓ | 0.052 | — |
| cursor parser throughput (MiB/s) ↑ | 254.9 (254.9–254.9) | — |
| cursor terminal CPU (s) ↓ | 0.045 | — |
| latency DSR p50 (ms) ↓ | 0.114 | — |
| latency DSR p95 (ms) ↓ | 0.163 | — |
| latency DSR p99 (ms) ↓ | 0.239 | — |
| loaded_latency DSR p50 (ms) ↓ | 0.144 | — |
| loaded_latency DSR p95 (ms) ↓ | 0.192 | — |
| loaded_latency DSR p99 (ms) ↓ | 0.212 | — |
| tui_full DSR p50 (ms) ↓ | 0.415 | — |
| tui_full DSR p95 (ms) ↓ | 0.657 | — |
| tui_full DSR p99 (ms) ↓ | 0.868 | — |
| tui_full terminal CPU (%) ↓ | 14.4 | — |
| tui_full parser deadline misses ↓ | 0 | — |
| tui_partial DSR p50 (ms) ↓ | 0.223 | — |
| tui_partial DSR p95 (ms) ↓ | 0.365 | — |
| tui_partial DSR p99 (ms) ↓ | 0.571 | — |
| tui_partial terminal CPU (%) ↓ | 9.4 | — |
| tui_partial parser deadline misses ↓ | 0 | — |
| Peak sampled RSS (MiB) ↓ | 183.2 | — |
| Peak sampled physical footprint (MiB) ↓ | 152.2 | — |

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

## Failed runs (not included in medians)

- after round 0: concurrent Rust build/test processes detected: 17563, 17564; retry when idle
