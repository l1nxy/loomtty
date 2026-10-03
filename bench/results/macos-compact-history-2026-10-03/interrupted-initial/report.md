# macOS terminal benchmark

Generated: 2026-10-03T14:43:17.940691+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | — | — |
| Launch → first DSR response (ms) ↓ | — | — |
| Cold idle RSS (MiB) ↓ | — | — |
| Cold idle physical footprint (MiB) ↓ | — | — |
| Cold idle CPU (% of one core) ↓ | — | — |
| Warm idle RSS (MiB) ↓ | — | — |
| Warm idle physical footprint (MiB) ↓ | — | — |
| Warm idle CPU (% of one core) ↓ | — | — |
| After history RSS (MiB) ↓ | — | — |
| After history physical footprint (MiB) ↓ | — | — |
| After history CPU (% of one core) ↓ | — | — |
| ascii parser throughput (MiB/s) ↑ | — | — |
| ascii terminal CPU (s) ↓ | — | — |
| truecolor parser throughput (MiB/s) ↑ | — | — |
| truecolor terminal CPU (s) ↓ | — | — |
| cjk parser throughput (MiB/s) ↑ | — | — |
| cjk terminal CPU (s) ↓ | — | — |
| emoji parser throughput (MiB/s) ↑ | — | — |
| emoji terminal CPU (s) ↓ | — | — |
| scroll parser throughput (MiB/s) ↑ | — | — |
| scroll terminal CPU (s) ↓ | — | — |
| cursor parser throughput (MiB/s) ↑ | — | — |
| cursor terminal CPU (s) ↓ | — | — |
| latency DSR p50 (ms) ↓ | — | — |
| latency DSR p95 (ms) ↓ | — | — |
| latency DSR p99 (ms) ↓ | — | — |
| loaded_latency DSR p50 (ms) ↓ | — | — |
| loaded_latency DSR p95 (ms) ↓ | — | — |
| loaded_latency DSR p99 (ms) ↓ | — | — |
| tui_full DSR p50 (ms) ↓ | — | — |
| tui_full DSR p95 (ms) ↓ | — | — |
| tui_full DSR p99 (ms) ↓ | — | — |
| tui_full terminal CPU (%) ↓ | — | — |
| tui_full parser deadline misses ↓ | — | — |
| tui_partial DSR p50 (ms) ↓ | — | — |
| tui_partial DSR p95 (ms) ↓ | — | — |
| tui_partial DSR p99 (ms) ↓ | — | — |
| tui_partial terminal CPU (%) ↓ | — | — |
| tui_partial parser deadline misses ↓ | — | — |
| Peak sampled RSS (MiB) ↓ | — | — |
| Peak sampled physical footprint (MiB) ↓ | — | — |

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
