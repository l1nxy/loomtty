# macOS terminal benchmark

Generated: 2026-10-02T02:08:12.406607+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 2 | 2 |
| Launch → first DSR response (ms) ↓ | 716.1 (602.9–829.3) | 660.0 (610.9–709.1) |
| Cold idle RSS (MiB) ↓ | 109.4 | 109.4 |
| Cold idle physical footprint (MiB) ↓ | 66.7 | 66.7 |
| Cold idle CPU (% of one core) ↓ | 0.02 | 0.02 |
| Warm idle RSS (MiB) ↓ | 116.1 | 116.3 |
| Warm idle physical footprint (MiB) ↓ | 83.6 | 74.7 |
| Warm idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| After history RSS (MiB) ↓ | 192.6 | 192.8 |
| After history physical footprint (MiB) ↓ | 155.9 | 164.1 |
| After history CPU (% of one core) ↓ | 0.07 | 0.03 |
| ascii parser throughput (MiB/s) ↑ | 201.0 (198.6–203.5) | 194.1 (193.9–194.3) |
| ascii terminal CPU (s) ↓ | 0.004 | 0.004 |
| truecolor parser throughput (MiB/s) ↑ | 215.5 (214.7–216.4) | 220.2 (216.3–224.1) |
| truecolor terminal CPU (s) ↓ | 0.003 | 0.003 |
| cjk parser throughput (MiB/s) ↑ | 214.0 (205.3–222.7) | 221.9 (218.2–225.6) |
| cjk terminal CPU (s) ↓ | 0.003 | 0.003 |
| emoji parser throughput (MiB/s) ↑ | 220.1 (217.3–222.9) | 222.9 (214.2–231.6) |
| emoji terminal CPU (s) ↓ | 0.003 | 0.003 |
| scroll parser throughput (MiB/s) ↑ | 175.8 (171.7–180.0) | 179.9 (177.1–182.6) |
| scroll terminal CPU (s) ↓ | 0.003 | 0.003 |
| cursor parser throughput (MiB/s) ↑ | 203.5 (203.1–204.0) | 197.9 (197.7–198.1) |
| cursor terminal CPU (s) ↓ | 0.003 | 0.003 |
| latency DSR p50 (ms) ↓ | 0.047 | 0.045 |
| latency DSR p95 (ms) ↓ | 0.088 | 0.089 |
| latency DSR p99 (ms) ↓ | 0.136 | 0.141 |
| loaded_latency DSR p50 (ms) ↓ | 0.163 | 0.150 |
| loaded_latency DSR p95 (ms) ↓ | 0.184 | 0.188 |
| loaded_latency DSR p99 (ms) ↓ | 0.199 | 0.203 |
| tui_full DSR p50 (ms) ↓ | 0.301 | 0.301 |
| tui_full DSR p95 (ms) ↓ | 0.477 | 0.488 |
| tui_full DSR p99 (ms) ↓ | 0.543 | 0.563 |
| tui_full terminal CPU (%) ↓ | 11.6 | 11.6 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.118 | 0.127 |
| tui_partial DSR p95 (ms) ↓ | 0.185 | 0.193 |
| tui_partial DSR p99 (ms) ↓ | 0.229 | 0.218 |
| tui_partial terminal CPU (%) ↓ | 7.0 | 6.8 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 192.6 | 192.8 |
| Peak sampled physical footprint (MiB) ↓ | 164.9 | 165.1 |

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
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
- after: `loomtty 0.2.1`; binary SHA-256 `e9116146b306509b907816c2b28c2b9e050c5a3928192587ddad36accc84ac40`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
