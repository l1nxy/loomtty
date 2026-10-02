# macOS terminal benchmark

Generated: 2026-10-02T01:55:36.144370+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 625.0 (466.8–849.1) | 529.4 (485.8–530.6) |
| Cold idle RSS (MiB) ↓ | 109.2 | 109.3 |
| Cold idle physical footprint (MiB) ↓ | 66.5 | 66.6 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.03 |
| Warm idle RSS (MiB) ↓ | 116.1 | 116.2 |
| Warm idle physical footprint (MiB) ↓ | 68.3 | 68.3 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.02 |
| After history RSS (MiB) ↓ | 192.7 | 192.7 |
| After history physical footprint (MiB) ↓ | 144.8 | 144.7 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 251.2 (247.8–258.0) | 256.0 (249.5–257.5) |
| ascii terminal CPU (s) ↓ | 0.045 | 0.045 |
| truecolor parser throughput (MiB/s) ↑ | 300.8 (298.4–314.4) | 309.8 (304.5–312.7) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 313.5 (308.4–321.8) | 315.9 (315.4–320.3) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.039 |
| emoji parser throughput (MiB/s) ↑ | 341.9 (339.9–359.9) | 351.1 (340.7–362.3) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.037 |
| scroll parser throughput (MiB/s) ↑ | 216.1 (214.7–217.0) | 216.8 (216.7–218.2) |
| scroll terminal CPU (s) ↓ | 0.050 | 0.050 |
| cursor parser throughput (MiB/s) ↑ | 251.8 (250.6–257.1) | 256.8 (255.6–260.4) |
| cursor terminal CPU (s) ↓ | 0.044 | 0.045 |
| latency DSR p50 (ms) ↓ | 0.086 | 0.086 |
| latency DSR p95 (ms) ↓ | 0.120 | 0.128 |
| latency DSR p99 (ms) ↓ | 0.148 | 0.155 |
| loaded_latency DSR p50 (ms) ↓ | 0.141 | 0.140 |
| loaded_latency DSR p95 (ms) ↓ | 0.186 | 0.170 |
| loaded_latency DSR p99 (ms) ↓ | 0.209 | 0.203 |
| tui_full DSR p50 (ms) ↓ | 0.314 | 0.302 |
| tui_full DSR p95 (ms) ↓ | 0.477 | 0.466 |
| tui_full DSR p99 (ms) ↓ | 0.549 | 0.555 |
| tui_full terminal CPU (%) ↓ | 11.5 | 11.0 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.126 | 0.125 |
| tui_partial DSR p95 (ms) ↓ | 0.187 | 0.191 |
| tui_partial DSR p99 (ms) ↓ | 0.229 | 0.226 |
| tui_partial terminal CPU (%) ↓ | 6.6 | 6.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 192.7 | 192.7 |
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

- before: `loomtty 0.2.1`; binary SHA-256 `ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
- after: `loomtty 0.2.1`; binary SHA-256 `bd09c5936127a337f3bcfe6bcc87231d16488b7a3d0d8a9a18e8d21fdade81dd`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
