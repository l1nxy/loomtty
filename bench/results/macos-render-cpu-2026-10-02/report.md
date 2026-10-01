# macOS terminal benchmark

Generated: 2026-10-01T16:41:36.338467+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 643.0 (426.5–700.7) | 638.3 (610.9–766.5) |
| Cold idle RSS (MiB) ↓ | 108.3 | 108.4 |
| Cold idle physical footprint (MiB) ↓ | 66.4 | 66.4 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| Warm idle RSS (MiB) ↓ | 114.8 | 115.3 |
| Warm idle physical footprint (MiB) ↓ | 67.7 | 68.2 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 191.4 | 192.0 |
| After history physical footprint (MiB) ↓ | 144.3 | 144.7 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 239.4 (234.9–240.6) | 240.8 (240.4–242.3) |
| ascii terminal CPU (s) ↓ | 0.048 | 0.047 |
| truecolor parser throughput (MiB/s) ↑ | 289.5 (287.5–299.6) | 282.6 (260.3–294.6) |
| truecolor terminal CPU (s) ↓ | 0.042 | 0.042 |
| cjk parser throughput (MiB/s) ↑ | 297.3 (293.4–297.9) | 296.9 (291.2–299.0) |
| cjk terminal CPU (s) ↓ | 0.041 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 324.0 (250.5–330.1) | 329.0 (249.2–337.1) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 200.7 (200.5–205.2) | 204.2 (197.9–204.7) |
| scroll terminal CPU (s) ↓ | 0.053 | 0.053 |
| cursor parser throughput (MiB/s) ↑ | 242.0 (237.7–242.4) | 240.8 (240.6–245.1) |
| cursor terminal CPU (s) ↓ | 0.046 | 0.047 |
| latency DSR p50 (ms) ↓ | 0.084 | 0.083 |
| latency DSR p95 (ms) ↓ | 0.122 | 0.133 |
| latency DSR p99 (ms) ↓ | 0.156 | 0.154 |
| loaded_latency DSR p50 (ms) ↓ | 0.147 | 0.145 |
| loaded_latency DSR p95 (ms) ↓ | 0.181 | 0.175 |
| loaded_latency DSR p99 (ms) ↓ | 0.219 | 0.208 |
| tui_full DSR p50 (ms) ↓ | 0.343 | 0.343 |
| tui_full DSR p95 (ms) ↓ | 0.568 | 0.553 |
| tui_full DSR p99 (ms) ↓ | 0.667 | 0.664 |
| tui_full terminal CPU (%) ↓ | 12.9 | 11.9 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.127 | 0.119 |
| tui_partial DSR p95 (ms) ↓ | 0.185 | 0.188 |
| tui_partial DSR p99 (ms) ↓ | 0.235 | 0.232 |
| tui_partial terminal CPU (%) ↓ | 7.0 | 6.7 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 191.5 | 192.0 |
| Peak sampled physical footprint (MiB) ↓ | 164.5 | 165.0 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `7fb11f852f4d41eb874124c430d35844c75ccb2566ab1123cf6fe057f140792c`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- after: `loomtty 0.2.1`; binary SHA-256 `84d6e6bdfea8a69704dd05d3ae3baf833da82c145c3d35310aa79bb5c8d6b9e7`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
