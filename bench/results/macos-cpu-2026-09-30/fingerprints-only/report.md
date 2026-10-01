# macOS terminal benchmark

Generated: 2026-09-30T01:24:59.964549+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 921.0 (455.7–1093.4) | 858.5 (826.7–1552.4) |
| Cold idle RSS (MiB) ↓ | 106.7 | 106.6 |
| Cold idle physical footprint (MiB) ↓ | 67.3 | 61.6 |
| Cold idle CPU (% of one core) ↓ | 0.41 | 0.03 |
| Warm idle RSS (MiB) ↓ | 114.0 | 111.9 |
| Warm idle physical footprint (MiB) ↓ | 66.5 | 66.6 |
| Warm idle CPU (% of one core) ↓ | 0.02 | 0.02 |
| After history RSS (MiB) ↓ | 160.0 | 157.8 |
| After history physical footprint (MiB) ↓ | 115.9 | 116.2 |
| After history CPU (% of one core) ↓ | 0.01 | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 213.0 (208.2–213.8) | 228.2 (204.1–242.1) |
| ascii terminal CPU (s) ↓ | 0.053 | 0.051 |
| truecolor parser throughput (MiB/s) ↑ | 258.6 (253.5–261.7) | 295.2 (289.4–296.8) |
| truecolor terminal CPU (s) ↓ | 0.045 | 0.041 |
| cjk parser throughput (MiB/s) ↑ | 261.2 (255.6–262.9) | 296.0 (288.1–313.6) |
| cjk terminal CPU (s) ↓ | 0.045 | 0.042 |
| emoji parser throughput (MiB/s) ↑ | 286.0 (285.4–295.8) | 334.7 (327.1–340.0) |
| emoji terminal CPU (s) ↓ | 0.043 | 0.038 |
| scroll parser throughput (MiB/s) ↑ | 185.0 (184.0–186.8) | 197.5 (196.7–202.6) |
| scroll terminal CPU (s) ↓ | 0.057 | 0.054 |
| cursor parser throughput (MiB/s) ↑ | 211.0 (202.6–212.7) | 234.2 (224.3–234.2) |
| cursor terminal CPU (s) ↓ | 0.052 | 0.051 |
| latency DSR p50 (ms) ↓ | 0.148 | 0.143 |
| latency DSR p95 (ms) ↓ | 0.285 | 0.279 |
| latency DSR p99 (ms) ↓ | 0.516 | 0.647 |
| loaded_latency DSR p50 (ms) ↓ | 0.155 | 0.145 |
| loaded_latency DSR p95 (ms) ↓ | 0.233 | 0.214 |
| loaded_latency DSR p99 (ms) ↓ | 0.272 | 0.258 |
| tui_full DSR p50 (ms) ↓ | 0.712 | 0.471 |
| tui_full DSR p95 (ms) ↓ | 1.145 | 0.875 |
| tui_full DSR p99 (ms) ↓ | 1.456 | 1.020 |
| tui_full terminal CPU (%) ↓ | 15.8 | 14.1 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.344 | 0.251 |
| tui_partial DSR p95 (ms) ↓ | 1.132 | 0.751 |
| tui_partial DSR p99 (ms) ↓ | 1.484 | 1.281 |
| tui_partial terminal CPU (%) ↓ | 9.4 | 9.6 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 160.0 | 157.8 |
| Peak sampled physical footprint (MiB) ↓ | 136.1 | 136.5 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.1.0`; binary SHA-256 `c5e7470c783b76b8f4cdca8300d1429424a54cfc6ba30be826310281fbed154f`
  - loomtty-server SHA-256 `74cc781612c2c61e134d94fca24b60124affed7d168adfc19b8c740f1c7d8737`
- after: `loomtty 0.1.0`; binary SHA-256 `c5cdae6e76b9553e9f5c882ac9f5ef7bd5c965b1c8a7bc36b7aa6054cae2ccdf`
  - loomtty-server SHA-256 `938508140ed935e52a4f7803ee7cb46a4ee7026bf63767bdaf0f1d65c35e587d`
