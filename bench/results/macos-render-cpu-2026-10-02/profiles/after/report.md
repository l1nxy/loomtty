# macOS terminal benchmark

Generated: 2026-10-01T16:46:40.178876+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty |
|---|---:|
| Successful launches | 1 |
| Launch → first DSR response (ms) ↓ | 552.6 (552.6–552.6) |
| Cold idle RSS (MiB) ↓ | 108.6 |
| Cold idle physical footprint (MiB) ↓ | 66.6 |
| Cold idle CPU (% of one core) ↓ | 0.02 |
| Warm idle RSS (MiB) ↓ | 115.5 |
| Warm idle physical footprint (MiB) ↓ | 92.7 |
| Warm idle CPU (% of one core) ↓ | 0.03 |
| After history RSS (MiB) ↓ | 192.2 |
| After history physical footprint (MiB) ↓ | 147.2 |
| After history CPU (% of one core) ↓ | 0.04 |
| ascii parser throughput (MiB/s) ↑ | 188.3 (188.3–188.3) |
| ascii terminal CPU (s) ↓ | 0.003 |
| truecolor parser throughput (MiB/s) ↑ | 208.0 (208.0–208.0) |
| truecolor terminal CPU (s) ↓ | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 218.2 (218.2–218.2) |
| cjk terminal CPU (s) ↓ | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 223.5 (223.5–223.5) |
| emoji terminal CPU (s) ↓ | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 169.2 (169.2–169.2) |
| scroll terminal CPU (s) ↓ | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 189.1 (189.1–189.1) |
| cursor terminal CPU (s) ↓ | 0.003 |
| latency DSR p50 (ms) ↓ | 0.047 |
| latency DSR p95 (ms) ↓ | 0.124 |
| latency DSR p99 (ms) ↓ | 0.145 |
| loaded_latency DSR p50 (ms) ↓ | 0.164 |
| loaded_latency DSR p95 (ms) ↓ | 0.211 |
| loaded_latency DSR p99 (ms) ↓ | 0.214 |
| tui_full DSR p50 (ms) ↓ | 0.331 |
| tui_full DSR p95 (ms) ↓ | 1.409 |
| tui_full DSR p99 (ms) ↓ | 2.073 |
| tui_full terminal CPU (%) ↓ | 10.1 |
| tui_full parser deadline misses ↓ | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.114 |
| tui_partial DSR p95 (ms) ↓ | 0.475 |
| tui_partial DSR p99 (ms) ↓ | 1.900 |
| tui_partial terminal CPU (%) ↓ | 5.8 |
| tui_partial parser deadline misses ↓ | 0 |
| Peak sampled RSS (MiB) ↓ | 192.2 |
| Peak sampled physical footprint (MiB) ↓ | 165.2 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- loomtty: `loomtty 0.2.1`; binary SHA-256 `84d6e6bdfea8a69704dd05d3ae3baf833da82c145c3d35310aa79bb5c8d6b9e7`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
