# macOS terminal benchmark

Generated: 2026-10-02T01:33:50.348805+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 2 | 2 |
| Launch → first DSR response (ms) ↓ | 813.3 (669.0–957.6) | 616.0 (611.6–620.5) |
| Cold idle RSS (MiB) ↓ | 109.3 | 109.2 |
| Cold idle physical footprint (MiB) ↓ | 66.5 | 66.5 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.02 |
| Warm idle RSS (MiB) ↓ | 115.9 | 115.9 |
| Warm idle physical footprint (MiB) ↓ | 74.3 | 83.3 |
| Warm idle CPU (% of one core) ↓ | 0.05 | 0.04 |
| After history RSS (MiB) ↓ | 192.5 | 192.5 |
| After history physical footprint (MiB) ↓ | 155.7 | 154.6 |
| After history CPU (% of one core) ↓ | 0.10 | 0.05 |
| ascii parser throughput (MiB/s) ↑ | 189.5 (189.2–189.8) | 166.8 (135.6–197.9) |
| ascii terminal CPU (s) ↓ | 0.004 | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 204.8 (198.1–211.6) | 219.5 (216.6–222.3) |
| truecolor terminal CPU (s) ↓ | 0.003 | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 217.8 (215.5–220.1) | 224.3 (216.6–232.1) |
| cjk terminal CPU (s) ↓ | 0.003 | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 210.8 (205.2–216.4) | 225.1 (224.2–226.0) |
| emoji terminal CPU (s) ↓ | 0.003 | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 171.8 (171.3–172.3) | 174.8 (167.8–181.8) |
| scroll terminal CPU (s) ↓ | 0.003 | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 191.0 (188.2–193.8) | 173.7 (138.0–209.4) |
| cursor terminal CPU (s) ↓ | 0.003 | 0.003 |
| latency DSR p50 (ms) ↓ | 0.046 | 0.046 |
| latency DSR p95 (ms) ↓ | 0.086 | 0.071 |
| latency DSR p99 (ms) ↓ | 0.142 | 0.137 |
| loaded_latency DSR p50 (ms) ↓ | 0.158 | 0.162 |
| loaded_latency DSR p95 (ms) ↓ | 0.191 | 0.199 |
| loaded_latency DSR p99 (ms) ↓ | 0.209 | 0.202 |
| tui_full DSR p50 (ms) ↓ | 0.357 | 0.304 |
| tui_full DSR p95 (ms) ↓ | 0.584 | 0.466 |
| tui_full DSR p99 (ms) ↓ | 0.700 | 0.526 |
| tui_full terminal CPU (%) ↓ | 12.5 | 11.7 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.118 | 0.125 |
| tui_partial DSR p95 (ms) ↓ | 0.186 | 0.187 |
| tui_partial DSR p99 (ms) ↓ | 0.242 | 0.235 |
| tui_partial terminal CPU (%) ↓ | 7.1 | 7.0 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 192.5 | 192.5 |
| Peak sampled physical footprint (MiB) ↓ | 164.7 | 164.7 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `07e4bfd8e8a53138198c1b0add29e5123d62e981e758aa09d7168f301d5f6734`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- after: `loomtty 0.2.1`; binary SHA-256 `07e4bfd8e8a53138198c1b0add29e5123d62e981e758aa09d7168f301d5f6734`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
