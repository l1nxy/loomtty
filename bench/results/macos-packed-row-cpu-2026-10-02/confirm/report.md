# macOS terminal benchmark

Generated: 2026-10-02T01:36:54.939904+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 2 | 2 |
| Launch → first DSR response (ms) ↓ | 608.5 (592.7–624.4) | 792.9 (623.3–962.5) |
| Cold idle RSS (MiB) ↓ | 109.3 | 109.4 |
| Cold idle physical footprint (MiB) ↓ | 66.6 | 66.7 |
| Cold idle CPU (% of one core) ↓ | 0.01 | 0.02 |
| Warm idle RSS (MiB) ↓ | 116.0 | 116.1 |
| Warm idle physical footprint (MiB) ↓ | 84.4 | 84.6 |
| Warm idle CPU (% of one core) ↓ | 0.05 | 0.04 |
| After history RSS (MiB) ↓ | 192.6 | 192.7 |
| After history physical footprint (MiB) ↓ | 146.8 | 155.9 |
| After history CPU (% of one core) ↓ | 0.03 | 0.03 |
| ascii parser throughput (MiB/s) ↑ | 188.7 (186.3–191.1) | 197.8 (191.6–204.0) |
| ascii terminal CPU (s) ↓ | 0.004 | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 213.8 (213.2–214.5) | 219.7 (216.7–222.7) |
| truecolor terminal CPU (s) ↓ | 0.003 | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 211.2 (210.6–211.8) | 217.9 (216.3–219.4) |
| cjk terminal CPU (s) ↓ | 0.003 | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 220.0 (217.3–222.7) | 211.7 (200.6–222.8) |
| emoji terminal CPU (s) ↓ | 0.003 | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 171.1 (167.8–174.3) | 172.7 (163.6–181.9) |
| scroll terminal CPU (s) ↓ | 0.003 | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 191.0 (187.9–194.1) | 200.9 (199.6–202.3) |
| cursor terminal CPU (s) ↓ | 0.003 | 0.003 |
| latency DSR p50 (ms) ↓ | 0.047 | 0.046 |
| latency DSR p95 (ms) ↓ | 0.104 | 0.095 |
| latency DSR p99 (ms) ↓ | 0.134 | 0.148 |
| loaded_latency DSR p50 (ms) ↓ | 0.166 | 0.160 |
| loaded_latency DSR p95 (ms) ↓ | 0.196 | 0.196 |
| loaded_latency DSR p99 (ms) ↓ | 0.212 | 0.218 |
| tui_full DSR p50 (ms) ↓ | 0.356 | 0.304 |
| tui_full DSR p95 (ms) ↓ | 0.580 | 0.486 |
| tui_full DSR p99 (ms) ↓ | 0.703 | 0.568 |
| tui_full terminal CPU (%) ↓ | 12.5 | 11.7 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.119 | 0.126 |
| tui_partial DSR p95 (ms) ↓ | 0.184 | 0.193 |
| tui_partial DSR p99 (ms) ↓ | 0.237 | 0.228 |
| tui_partial terminal CPU (%) ↓ | 6.8 | 6.7 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 192.6 | 192.7 |
| Peak sampled physical footprint (MiB) ↓ | 164.8 | 164.9 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- after: `loomtty 0.2.1`; binary SHA-256 `ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
