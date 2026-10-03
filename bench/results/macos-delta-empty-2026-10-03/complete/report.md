# macOS terminal benchmark

Generated: 2026-10-03T15:45:04.727228+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 610.7 (467.8–956.0) | 1043.5 (641.9–2577.8) |
| Cold idle RSS (MiB) ↓ | 114.9 | 114.8 |
| Cold idle physical footprint (MiB) ↓ | 68.0 | 68.0 |
| Cold idle CPU (% of one core) ↓ | 0.05 | 0.04 |
| Warm idle RSS (MiB) ↓ | 121.8 | 122.1 |
| Warm idle physical footprint (MiB) ↓ | 70.0 | 70.1 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.02 |
| After history RSS (MiB) ↓ | 183.6 | 183.6 |
| After history physical footprint (MiB) ↓ | 132.4 | 132.3 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 254.3 (253.8–256.3) | 255.3 (246.4–257.6) |
| ascii terminal CPU (s) ↓ | 0.045 | 0.045 |
| truecolor parser throughput (MiB/s) ↑ | 300.8 (300.7–307.8) | 292.9 (291.4–308.0) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.042 |
| cjk parser throughput (MiB/s) ↑ | 307.9 (251.5–315.8) | 311.9 (297.7–322.2) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.039 |
| emoji parser throughput (MiB/s) ↑ | 335.1 (317.4–345.9) | 337.0 (334.5–348.5) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 208.0 (201.3–214.7) | 211.7 (207.5–214.7) |
| scroll terminal CPU (s) ↓ | 0.052 | 0.051 |
| cursor parser throughput (MiB/s) ↑ | 256.8 (256.3–258.0) | 250.4 (242.9–256.0) |
| cursor terminal CPU (s) ↓ | 0.044 | 0.045 |
| latency DSR p50 (ms) ↓ | 0.111 | 0.115 |
| latency DSR p95 (ms) ↓ | 0.160 | 0.180 |
| latency DSR p99 (ms) ↓ | 0.211 | 0.248 |
| loaded_latency DSR p50 (ms) ↓ | 0.144 | 0.144 |
| loaded_latency DSR p95 (ms) ↓ | 0.188 | 0.186 |
| loaded_latency DSR p99 (ms) ↓ | 0.212 | 0.218 |
| tui_full DSR p50 (ms) ↓ | 0.419 | 0.424 |
| tui_full DSR p95 (ms) ↓ | 0.691 | 0.633 |
| tui_full DSR p99 (ms) ↓ | 1.084 | 0.783 |
| tui_full terminal CPU (%) ↓ | 14.5 | 13.9 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.227 | 0.215 |
| tui_partial DSR p95 (ms) ↓ | 0.467 | 0.282 |
| tui_partial DSR p99 (ms) ↓ | 0.737 | 0.410 |
| tui_partial terminal CPU (%) ↓ | 9.4 | 9.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.6 | 183.6 |
| Peak sampled physical footprint (MiB) ↓ | 152.6 | 152.5 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `a8ec39413dda7bf464a5ac337ff774ba9ba48b559cbee4f47d0a78138dc4aedc`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- after: `loomtty 0.2.1`; binary SHA-256 `70be8ddafc7b56a1ea25456bace73ba60dc30f0f5a1098ab6aefba65d5151f3a`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
