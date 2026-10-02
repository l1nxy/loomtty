# macOS terminal benchmark

Generated: 2026-10-02T01:26:41.207700+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 657.8 (643.6–1050.1) | 930.8 (621.9–1134.5) |
| Cold idle RSS (MiB) ↓ | 109.0 | 109.2 |
| Cold idle physical footprint (MiB) ↓ | 66.2 | 66.5 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| Warm idle RSS (MiB) ↓ | 115.9 | 116.2 |
| Warm idle physical footprint (MiB) ↓ | 68.0 | 68.3 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 192.5 | 192.7 |
| After history physical footprint (MiB) ↓ | 144.6 | 144.8 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 239.0 (227.8–242.6) | 255.5 (255.0–256.8) |
| ascii terminal CPU (s) ↓ | 0.048 | 0.046 |
| truecolor parser throughput (MiB/s) ↑ | 286.5 (285.9–289.8) | 310.4 (298.9–317.7) |
| truecolor terminal CPU (s) ↓ | 0.042 | 0.039 |
| cjk parser throughput (MiB/s) ↑ | 293.6 (292.1–298.7) | 309.2 (251.2–311.9) |
| cjk terminal CPU (s) ↓ | 0.041 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 313.1 (267.8–327.9) | 335.2 (329.6–345.8) |
| emoji terminal CPU (s) ↓ | 0.041 | 0.039 |
| scroll parser throughput (MiB/s) ↑ | 202.6 (202.4–204.3) | 207.8 (200.3–211.9) |
| scroll terminal CPU (s) ↓ | 0.053 | 0.052 |
| cursor parser throughput (MiB/s) ↑ | 241.7 (229.6–245.3) | 256.9 (256.3–258.4) |
| cursor terminal CPU (s) ↓ | 0.046 | 0.044 |
| latency DSR p50 (ms) ↓ | 0.086 | 0.085 |
| latency DSR p95 (ms) ↓ | 0.136 | 0.129 |
| latency DSR p99 (ms) ↓ | 0.172 | 0.145 |
| loaded_latency DSR p50 (ms) ↓ | 0.150 | 0.145 |
| loaded_latency DSR p95 (ms) ↓ | 0.188 | 0.181 |
| loaded_latency DSR p99 (ms) ↓ | 0.218 | 0.207 |
| tui_full DSR p50 (ms) ↓ | 0.350 | 0.306 |
| tui_full DSR p95 (ms) ↓ | 0.560 | 0.484 |
| tui_full DSR p99 (ms) ↓ | 0.669 | 0.573 |
| tui_full terminal CPU (%) ↓ | 12.3 | 11.5 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.126 | 0.128 |
| tui_partial DSR p95 (ms) ↓ | 0.198 | 0.198 |
| tui_partial DSR p99 (ms) ↓ | 0.239 | 0.239 |
| tui_partial terminal CPU (%) ↓ | 6.1 | 6.8 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 192.6 | 192.7 |
| Peak sampled physical footprint (MiB) ↓ | 164.9 | 165.0 |

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
