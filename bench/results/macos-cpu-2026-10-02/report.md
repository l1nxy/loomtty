# macOS terminal benchmark

Generated: 2026-10-01T16:17:33.657012+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 508.8 (466.9–605.8) | 922.9 (612.6–1224.0) |
| Cold idle RSS (MiB) ↓ | 108.4 | 108.2 |
| Cold idle physical footprint (MiB) ↓ | 66.8 | 66.8 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.03 |
| Warm idle RSS (MiB) ↓ | 115.0 | 114.8 |
| Warm idle physical footprint (MiB) ↓ | 68.4 | 68.2 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 191.5 | 191.4 |
| After history physical footprint (MiB) ↓ | 144.7 | 144.7 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 239.0 (235.1–243.0) | 236.2 (228.5–240.6) |
| ascii terminal CPU (s) ↓ | 0.048 | 0.048 |
| truecolor parser throughput (MiB/s) ↑ | 293.9 (277.7–297.2) | 285.1 (272.1–286.1) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.042 |
| cjk parser throughput (MiB/s) ↑ | 301.9 (292.6–303.5) | 298.6 (297.9–301.7) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 336.9 (332.1–343.3) | 330.2 (319.5–330.6) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.038 |
| scroll parser throughput (MiB/s) ↑ | 203.1 (200.4–204.3) | 205.4 (202.7–207.6) |
| scroll terminal CPU (s) ↓ | 0.052 | 0.052 |
| cursor parser throughput (MiB/s) ↑ | 240.5 (240.0–242.3) | 242.2 (235.4–244.4) |
| cursor terminal CPU (s) ↓ | 0.047 | 0.047 |
| latency DSR p50 (ms) ↓ | 0.080 | 0.079 |
| latency DSR p95 (ms) ↓ | 0.118 | 0.111 |
| latency DSR p99 (ms) ↓ | 0.141 | 0.138 |
| loaded_latency DSR p50 (ms) ↓ | 0.149 | 0.145 |
| loaded_latency DSR p95 (ms) ↓ | 0.182 | 0.175 |
| loaded_latency DSR p99 (ms) ↓ | 0.204 | 0.195 |
| tui_full DSR p50 (ms) ↓ | 0.322 | 0.321 |
| tui_full DSR p95 (ms) ↓ | 0.524 | 0.513 |
| tui_full DSR p99 (ms) ↓ | 0.581 | 0.573 |
| tui_full terminal CPU (%) ↓ | 13.4 | 12.3 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.108 | 0.109 |
| tui_partial DSR p95 (ms) ↓ | 0.156 | 0.157 |
| tui_partial DSR p99 (ms) ↓ | 0.196 | 0.213 |
| tui_partial terminal CPU (%) ↓ | 7.4 | 6.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 191.5 | 191.4 |
| Peak sampled physical footprint (MiB) ↓ | 165.0 | 165.0 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `26a47e68892414b2c4714ab0e1b4bfb19ea12b77266ecc24de5b3ed461048d66`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- after: `loomtty 0.2.1`; binary SHA-256 `7fb11f852f4d41eb874124c430d35844c75ccb2566ab1123cf6fe057f140792c`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
