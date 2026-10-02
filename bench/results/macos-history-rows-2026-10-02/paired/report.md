# macOS terminal benchmark

Generated: 2026-10-02T03:59:46.092030+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 595.6 (587.1–613.8) | 619.1 (609.3–866.5) |
| Cold idle RSS (MiB) ↓ | 116.8 | 117.4 |
| Cold idle physical footprint (MiB) ↓ | 61.3 | 62.7 |
| Cold idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| Warm idle RSS (MiB) ↓ | 123.6 | 124.3 |
| Warm idle physical footprint (MiB) ↓ | 68.0 | 69.2 |
| Warm idle CPU (% of one core) ↓ | 0.02 | 0.02 |
| After history RSS (MiB) ↓ | 200.0 | 185.6 |
| After history physical footprint (MiB) ↓ | 145.0 | 131.0 |
| After history CPU (% of one core) ↓ | 0.02 | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 255.8 (247.0–257.9) | 246.1 (241.4–250.1) |
| ascii terminal CPU (s) ↓ | 0.045 | 0.046 |
| truecolor parser throughput (MiB/s) ↑ | 312.3 (300.6–316.6) | 310.6 (296.6–311.0) |
| truecolor terminal CPU (s) ↓ | 0.040 | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 320.5 (317.1–323.0) | 316.3 (307.6–327.8) |
| cjk terminal CPU (s) ↓ | 0.039 | 0.039 |
| emoji parser throughput (MiB/s) ↑ | 340.6 (340.0–348.8) | 346.6 (340.4–351.1) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.037 |
| scroll parser throughput (MiB/s) ↑ | 215.8 (215.2–216.8) | 214.5 (207.3–216.3) |
| scroll terminal CPU (s) ↓ | 0.050 | 0.050 |
| cursor parser throughput (MiB/s) ↑ | 255.6 (253.0–257.7) | 255.6 (245.5–257.9) |
| cursor terminal CPU (s) ↓ | 0.045 | 0.044 |
| latency DSR p50 (ms) ↓ | 0.070 | 0.074 |
| latency DSR p95 (ms) ↓ | 0.121 | 0.126 |
| latency DSR p99 (ms) ↓ | 0.153 | 0.152 |
| loaded_latency DSR p50 (ms) ↓ | 0.143 | 0.142 |
| loaded_latency DSR p95 (ms) ↓ | 0.185 | 0.180 |
| loaded_latency DSR p99 (ms) ↓ | 0.212 | 0.212 |
| tui_full DSR p50 (ms) ↓ | 0.299 | 0.258 |
| tui_full DSR p95 (ms) ↓ | 0.457 | 0.412 |
| tui_full DSR p99 (ms) ↓ | 0.533 | 0.487 |
| tui_full terminal CPU (%) ↓ | 9.9 | 9.9 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.166 | 0.180 |
| tui_partial DSR p95 (ms) ↓ | 0.235 | 0.241 |
| tui_partial DSR p99 (ms) ↓ | 0.280 | 0.256 |
| tui_partial terminal CPU (%) ↓ | 7.0 | 6.9 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 200.0 | 185.6 |
| Peak sampled physical footprint (MiB) ↓ | 165.3 | 151.3 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `1b8f0d6ea1f2eaa59ec451df95c283e82cbb045ec193b5ad8da049ff936aa6b0`
  - loomtty-server SHA-256 `2d6d438e8998523a4f417a2334b55210bbc72647bd4121f8d284edbab2aeb8de`
- after: `loomtty 0.2.1`; binary SHA-256 `e8c11a399a3f46e2970ed519fb8128c48e0f9146e5f70068a4662ec18f0ca97c`
  - loomtty-server SHA-256 `2d6d438e8998523a4f417a2334b55210bbc72647bd4121f8d284edbab2aeb8de`
