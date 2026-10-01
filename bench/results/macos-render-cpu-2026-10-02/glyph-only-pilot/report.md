# macOS terminal benchmark

Generated: 2026-10-01T16:38:10.069003+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 1 | 1 |
| Launch → first DSR response (ms) ↓ | 985.8 (985.8–985.8) | 782.0 (782.0–782.0) |
| Cold idle RSS (MiB) ↓ | 108.6 | 108.5 |
| Cold idle physical footprint (MiB) ↓ | 66.7 | 66.6 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.01 |
| Warm idle RSS (MiB) ↓ | 115.0 | 114.9 |
| Warm idle physical footprint (MiB) ↓ | 74.2 | 94.1 |
| Warm idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| After history RSS (MiB) ↓ | 191.5 | 191.4 |
| After history physical footprint (MiB) ↓ | 162.6 | 146.4 |
| After history CPU (% of one core) ↓ | 0.02 | 0.03 |
| ascii parser throughput (MiB/s) ↑ | 188.2 (188.2–188.2) | 178.8 (178.8–178.8) |
| ascii terminal CPU (s) ↓ | 0.004 | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 210.2 (210.2–210.2) | 199.5 (199.5–199.5) |
| truecolor terminal CPU (s) ↓ | 0.003 | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 203.1 (203.1–203.1) | 212.9 (212.9–212.9) |
| cjk terminal CPU (s) ↓ | 0.003 | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 209.7 (209.7–209.7) | 216.4 (216.4–216.4) |
| emoji terminal CPU (s) ↓ | 0.003 | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 167.1 (167.1–167.1) | 167.3 (167.3–167.3) |
| scroll terminal CPU (s) ↓ | 0.003 | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 187.3 (187.3–187.3) | 187.3 (187.3–187.3) |
| cursor terminal CPU (s) ↓ | 0.003 | 0.003 |
| latency DSR p50 (ms) ↓ | 0.047 | 0.049 |
| latency DSR p95 (ms) ↓ | 0.118 | 0.144 |
| latency DSR p99 (ms) ↓ | 0.119 | 0.151 |
| loaded_latency DSR p50 (ms) ↓ | 0.163 | 0.163 |
| loaded_latency DSR p95 (ms) ↓ | 0.196 | 0.200 |
| loaded_latency DSR p99 (ms) ↓ | 0.204 | 0.216 |
| tui_full DSR p50 (ms) ↓ | 0.349 | 0.358 |
| tui_full DSR p95 (ms) ↓ | 0.557 | 0.576 |
| tui_full DSR p99 (ms) ↓ | 0.667 | 0.712 |
| tui_full terminal CPU (%) ↓ | 13.0 | 13.0 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.117 | 0.126 |
| tui_partial DSR p95 (ms) ↓ | 0.173 | 0.202 |
| tui_partial DSR p99 (ms) ↓ | 0.206 | 0.254 |
| tui_partial terminal CPU (%) ↓ | 6.9 | 7.0 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 191.5 | 191.4 |
| Peak sampled physical footprint (MiB) ↓ | 164.6 | 164.4 |

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
- after: `loomtty 0.2.1`; binary SHA-256 `79af46c1700d4ef5f3671f0dbe13f8e4cba68d05b043693561440aa3edeae3cc`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
