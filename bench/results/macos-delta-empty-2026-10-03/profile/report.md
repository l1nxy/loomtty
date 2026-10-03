# macOS terminal benchmark

Generated: 2026-10-03T15:53:51.577632+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 439.0 (439.0–439.0) |
| Cold idle RSS (MiB) ↓ | 114.9 |
| Cold idle physical footprint (MiB) ↓ | 68.0 |
| Cold idle CPU (% of one core) ↓ | 0.04 |
| Warm idle RSS (MiB) ↓ | 121.8 |
| Warm idle physical footprint (MiB) ↓ | 76.1 |
| Warm idle CPU (% of one core) ↓ | 0.04 |
| After history RSS (MiB) ↓ | 183.6 |
| After history physical footprint (MiB) ↓ | 134.6 |
| After history CPU (% of one core) ↓ | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 196.3 (196.3–196.3) |
| ascii terminal CPU (s) ↓ | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 213.7 (213.7–213.7) |
| truecolor terminal CPU (s) ↓ | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 206.1 (206.1–206.1) |
| cjk terminal CPU (s) ↓ | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 225.2 (225.2–225.2) |
| emoji terminal CPU (s) ↓ | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 178.0 (178.0–178.0) |
| scroll terminal CPU (s) ↓ | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 198.2 (198.2–198.2) |
| cursor terminal CPU (s) ↓ | 0.003 |
| latency DSR p50 (ms) ↓ | 0.069 |
| latency DSR p95 (ms) ↓ | 0.120 |
| latency DSR p99 (ms) ↓ | 0.155 |
| loaded_latency DSR p50 (ms) ↓ | 0.171 |
| loaded_latency DSR p95 (ms) ↓ | 0.207 |
| loaded_latency DSR p99 (ms) ↓ | 0.217 |
| tui_full DSR p50 (ms) ↓ | 0.430 |
| tui_full DSR p95 (ms) ↓ | 0.800 |
| tui_full DSR p99 (ms) ↓ | 1.265 |
| tui_full terminal CPU (%) ↓ | 13.3 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.180 |
| tui_partial DSR p95 (ms) ↓ | 0.268 |
| tui_partial DSR p99 (ms) ↓ | 0.350 |
| tui_partial terminal CPU (%) ↓ | 7.8 |
| tui_partial parser deadline misses ↓ | 0 |
| Peak sampled RSS (MiB) ↓ | 183.6 |
| Peak sampled physical footprint (MiB) ↓ | 152.6 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- loomtty: `loomtty 0.2.1`; binary SHA-256 `70be8ddafc7b56a1ea25456bace73ba60dc30f0f5a1098ab6aefba65d5151f3a`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
