# macOS terminal benchmark

Generated: 2026-10-01T16:55:55.249198+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 619.0 (595.5–978.4) | 807.1 (600.4–1344.7) |
| Cold idle RSS (MiB) ↓ | 108.4 | 108.3 |
| Cold idle physical footprint (MiB) ↓ | 66.4 | 66.2 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.03 |
| Warm idle RSS (MiB) ↓ | 115.3 | 115.3 |
| Warm idle physical footprint (MiB) ↓ | 68.2 | 68.0 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 191.8 | 191.9 |
| After history physical footprint (MiB) ↓ | 144.6 | 144.6 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 230.2 (230.0–232.9) | 229.5 (227.7–241.5) |
| ascii terminal CPU (s) ↓ | 0.049 | 0.049 |
| truecolor parser throughput (MiB/s) ↑ | 280.6 (275.8–286.3) | 289.0 (288.6–290.8) |
| truecolor terminal CPU (s) ↓ | 0.042 | 0.041 |
| cjk parser throughput (MiB/s) ↑ | 291.8 (288.8–299.7) | 290.2 (289.9–296.4) |
| cjk terminal CPU (s) ↓ | 0.041 | 0.041 |
| emoji parser throughput (MiB/s) ↑ | 327.5 (323.3–344.3) | 322.2 (321.0–332.8) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 200.7 (196.4–203.0) | 203.8 (200.5–204.9) |
| scroll terminal CPU (s) ↓ | 0.054 | 0.053 |
| cursor parser throughput (MiB/s) ↑ | 231.3 (228.7–235.2) | 244.9 (241.4–245.2) |
| cursor terminal CPU (s) ↓ | 0.049 | 0.046 |
| latency DSR p50 (ms) ↓ | 0.087 | 0.084 |
| latency DSR p95 (ms) ↓ | 0.125 | 0.142 |
| latency DSR p99 (ms) ↓ | 0.152 | 0.192 |
| loaded_latency DSR p50 (ms) ↓ | 0.146 | 0.148 |
| loaded_latency DSR p95 (ms) ↓ | 0.180 | 0.178 |
| loaded_latency DSR p99 (ms) ↓ | 0.205 | 0.212 |
| tui_full DSR p50 (ms) ↓ | 0.355 | 0.347 |
| tui_full DSR p95 (ms) ↓ | 0.581 | 0.559 |
| tui_full DSR p99 (ms) ↓ | 0.691 | 0.654 |
| tui_full terminal CPU (%) ↓ | 12.2 | 11.8 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.124 | 0.119 |
| tui_partial DSR p95 (ms) ↓ | 0.196 | 0.183 |
| tui_partial DSR p99 (ms) ↓ | 0.233 | 0.231 |
| tui_partial terminal CPU (%) ↓ | 6.8 | 6.6 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 191.8 | 191.9 |
| Peak sampled physical footprint (MiB) ↓ | 164.9 | 164.8 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `84d6e6bdfea8a69704dd05d3ae3baf833da82c145c3d35310aa79bb5c8d6b9e7`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- after: `loomtty 0.2.1`; binary SHA-256 `ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
