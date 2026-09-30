# macOS terminal benchmark

Generated: 2026-09-30T01:34:09.446044+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 807.1 (440.7–910.3) | 913.3 (852.7–1171.3) |
| Cold idle RSS (MiB) ↓ | 107.0 | 106.8 |
| Cold idle physical footprint (MiB) ↓ | 67.1 | 66.0 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.03 |
| Warm idle RSS (MiB) ↓ | 114.2 | 113.9 |
| Warm idle physical footprint (MiB) ↓ | 66.3 | 71.4 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 160.0 | 157.9 |
| After history physical footprint (MiB) ↓ | 116.2 | 115.8 |
| After history CPU (% of one core) ↓ | 0.02 | 0.02 |
| ascii parser throughput (MiB/s) ↑ | 214.2 (203.1–214.8) | 242.8 (237.1–247.4) |
| ascii terminal CPU (s) ↓ | 0.053 | 0.049 |
| truecolor parser throughput (MiB/s) ↑ | 242.9 (241.9–263.2) | 305.0 (304.5–305.7) |
| truecolor terminal CPU (s) ↓ | 0.048 | 0.040 |
| cjk parser throughput (MiB/s) ↑ | 266.6 (247.5–270.9) | 311.3 (310.6–322.5) |
| cjk terminal CPU (s) ↓ | 0.044 | 0.039 |
| emoji parser throughput (MiB/s) ↑ | 277.5 (267.9–298.8) | 336.9 (336.1–352.4) |
| emoji terminal CPU (s) ↓ | 0.043 | 0.038 |
| scroll parser throughput (MiB/s) ↑ | 185.0 (177.5–186.0) | 206.6 (202.1–206.9) |
| scroll terminal CPU (s) ↓ | 0.057 | 0.053 |
| cursor parser throughput (MiB/s) ↑ | 213.4 (200.5–214.2) | 243.8 (241.1–245.8) |
| cursor terminal CPU (s) ↓ | 0.052 | 0.047 |
| latency DSR p50 (ms) ↓ | 0.108 | 0.099 |
| latency DSR p95 (ms) ↓ | 0.141 | 0.133 |
| latency DSR p99 (ms) ↓ | 0.168 | 0.173 |
| loaded_latency DSR p50 (ms) ↓ | 0.154 | 0.142 |
| loaded_latency DSR p95 (ms) ↓ | 0.191 | 0.185 |
| loaded_latency DSR p99 (ms) ↓ | 0.238 | 0.211 |
| tui_full DSR p50 (ms) ↓ | 0.531 | 0.416 |
| tui_full DSR p95 (ms) ↓ | 0.915 | 0.749 |
| tui_full DSR p99 (ms) ↓ | 1.062 | 0.820 |
| tui_full terminal CPU (%) ↓ | 16.4 | 12.9 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.179 | 0.193 |
| tui_partial DSR p95 (ms) ↓ | 0.239 | 0.254 |
| tui_partial DSR p99 (ms) ↓ | 0.291 | 0.293 |
| tui_partial terminal CPU (%) ↓ | 8.5 | 8.5 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 160.1 | 158.0 |
| Peak sampled physical footprint (MiB) ↓ | 136.4 | 136.1 |

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
- after: `loomtty 0.1.0`; binary SHA-256 `cd46468ea41d275cfbb1e678b25910eea6bffb1c79dc42cfb2525c767b7c60c8`
  - loomtty-server SHA-256 `938508140ed935e52a4f7803ee7cb46a4ee7026bf63767bdaf0f1d65c35e587d`
