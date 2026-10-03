# macOS terminal benchmark

Generated: 2026-10-03T14:24:15.103735+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | before | after |
|---|---:|---:|
| Successful launches | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 460.4 (443.7–1337.5) | 1072.3 (1050.8–5586.1) |
| Cold idle RSS (MiB) ↓ | 114.8 | 114.9 |
| Cold idle physical footprint (MiB) ↓ | 67.9 | 67.9 |
| Cold idle CPU (% of one core) ↓ | 0.04 | 0.04 |
| Warm idle RSS (MiB) ↓ | 121.9 | 121.9 |
| Warm idle physical footprint (MiB) ↓ | 70.1 | 69.9 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.01 |
| After history RSS (MiB) ↓ | 183.5 | 183.6 |
| After history physical footprint (MiB) ↓ | 132.4 | 132.2 |
| After history CPU (% of one core) ↓ | 0.01 | 0.01 |
| ascii parser throughput (MiB/s) ↑ | 254.6 (241.0–255.8) | 255.8 (246.1–256.5) |
| ascii terminal CPU (s) ↓ | 0.047 | 0.046 |
| truecolor parser throughput (MiB/s) ↑ | 298.2 (297.1–300.1) | 299.1 (262.3–304.1) |
| truecolor terminal CPU (s) ↓ | 0.042 | 0.042 |
| cjk parser throughput (MiB/s) ↑ | 315.4 (304.5–320.6) | 312.7 (302.9–314.4) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.040 |
| emoji parser throughput (MiB/s) ↑ | 339.4 (316.6–339.4) | 338.1 (327.8–341.6) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.038 |
| scroll parser throughput (MiB/s) ↑ | 210.3 (209.1–214.7) | 211.1 (201.8–212.9) |
| scroll terminal CPU (s) ↓ | 0.052 | 0.053 |
| cursor parser throughput (MiB/s) ↑ | 248.3 (237.4–257.1) | 256.8 (256.2–257.9) |
| cursor terminal CPU (s) ↓ | 0.046 | 0.045 |
| latency DSR p50 (ms) ↓ | 0.107 | 0.106 |
| latency DSR p95 (ms) ↓ | 0.158 | 0.175 |
| latency DSR p99 (ms) ↓ | 0.226 | 0.272 |
| loaded_latency DSR p50 (ms) ↓ | 0.145 | 0.145 |
| loaded_latency DSR p95 (ms) ↓ | 0.186 | 0.183 |
| loaded_latency DSR p99 (ms) ↓ | 0.204 | 0.223 |
| tui_full DSR p50 (ms) ↓ | 0.409 | 0.446 |
| tui_full DSR p95 (ms) ↓ | 0.634 | 0.846 |
| tui_full DSR p99 (ms) ↓ | 0.850 | 1.259 |
| tui_full terminal CPU (%) ↓ | 14.4 | 14.7 |
| tui_full parser deadline misses ↓ | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.214 | 0.211 |
| tui_partial DSR p95 (ms) ↓ | 0.408 | 0.289 |
| tui_partial DSR p99 (ms) ↓ | 0.781 | 0.372 |
| tui_partial terminal CPU (%) ↓ | 9.6 | 9.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | 183.5 | 183.6 |
| Peak sampled physical footprint (MiB) ↓ | 152.7 | 152.4 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Before/after order alternates each round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- before: `loomtty 0.2.1`; binary SHA-256 `e467ffb2a2659a1a347461ae969cf5270393bb5091b2e1e55da7e1e59afc52af`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- after: `loomtty 0.2.1`; binary SHA-256 `47ca9ca14a9735ef3a05a262ed0b820708f233ce8995f63dc3f8f2591ce51b6e`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
