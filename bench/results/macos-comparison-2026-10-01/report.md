# macOS terminal benchmark

Generated: 2026-10-01T15:19:35.317705+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | 3 | 3 | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 951.0 (693.5–994.8) | 676.6 (672.2–1114.9) | 704.7 (527.9–754.9) | 886.2 (695.6–978.9) |
| Cold idle RSS (MiB) ↓ | 108.3 | 98.5 | 118.5 | 114.5 |
| Cold idle physical footprint (MiB) ↓ | 66.9 | 41.4 | 56.5 | 47.5 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.06 | 0.04 | 0.06 |
| Warm idle RSS (MiB) ↓ | 115.1 | 105.6 | 118.3 | 124.4 |
| Warm idle physical footprint (MiB) ↓ | 68.5 | 45.6 | 53.2 | 66.8 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.07 | 0.06 | 0.14 |
| After history RSS (MiB) ↓ | 191.5 | 132.5 | 152.1 | 133.7 |
| After history physical footprint (MiB) ↓ | 144.8 | 69.8 | 87.0 | 76.0 |
| After history CPU (% of one core) ↓ | 0.01 | 0.05 | 0.01 | 0.05 |
| ascii parser throughput (MiB/s) ↑ | 238.4 (237.6–241.0) | 201.1 (195.8–204.4) | 209.6 (198.8–209.7) | 116.3 (115.2–124.5) |
| ascii terminal CPU (s) ↓ | 0.048 | 0.044 | 0.031 | 0.137 |
| truecolor parser throughput (MiB/s) ↑ | 289.6 (284.0–296.0) | 174.7 (172.5–176.0) | 98.7 (95.3–99.3) | 115.9 (113.4–119.0) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.053 | 0.076 | 0.139 |
| cjk parser throughput (MiB/s) ↑ | 294.9 (289.2–304.0) | 179.8 (179.6–180.6) | 173.0 (169.0–174.4) | 116.6 (116.4–118.3) |
| cjk terminal CPU (s) ↓ | 0.041 | 0.053 | 0.039 | 0.135 |
| emoji parser throughput (MiB/s) ↑ | 323.5 (317.8–337.9) | 185.5 (183.2–194.7) | 189.3 (188.8–191.1) | 121.0 (120.6–122.4) |
| emoji terminal CPU (s) ↓ | 0.039 | 0.049 | 0.036 | 0.128 |
| scroll parser throughput (MiB/s) ↑ | 204.1 (202.4–205.6) | 200.0 (196.0–201.6) | 62.0 (62.0–62.1) | 67.9 (64.2–68.8) |
| scroll terminal CPU (s) ↓ | 0.053 | 0.042 | 0.123 | 0.220 |
| cursor parser throughput (MiB/s) ↑ | 236.9 (234.7–244.1) | 196.3 (195.1–200.3) | 164.8 (161.0–166.9) | 112.9 (111.7–114.4) |
| cursor terminal CPU (s) ↓ | 0.047 | 0.046 | 0.043 | 0.142 |
| latency DSR p50 (ms) ↓ | 0.081 | 0.113 | 3.215 | 0.098 |
| latency DSR p95 (ms) ↓ | 0.113 | 0.189 | 3.887 | 0.138 |
| latency DSR p99 (ms) ↓ | 0.138 | 0.742 | 3.925 | 0.186 |
| loaded_latency DSR p50 (ms) ↓ | 0.145 | 0.187 | 4.858 | 0.422 |
| loaded_latency DSR p95 (ms) ↓ | 0.179 | 0.225 | 5.233 | 0.517 |
| loaded_latency DSR p99 (ms) ↓ | 0.200 | 0.314 | 5.260 | 0.542 |
| tui_full DSR p50 (ms) ↓ | 0.321 | 0.221 | 3.211 | 0.326 |
| tui_full DSR p95 (ms) ↓ | 0.534 | 0.284 | 3.250 | 0.518 |
| tui_full DSR p99 (ms) ↓ | 0.584 | 0.404 | 3.283 | 0.575 |
| tui_full terminal CPU (%) ↓ | 12.8 | 8.5 | 2.9 | 14.4 |
| tui_full parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.105 | 0.134 | 3.909 | 0.115 |
| tui_partial DSR p95 (ms) ↓ | 0.159 | 0.191 | 190.720 | 0.166 |
| tui_partial DSR p99 (ms) ↓ | 0.213 | 0.306 | 191.557 | 0.210 |
| tui_partial terminal CPU (%) ↓ | 7.1 | 8.2 | 0.3 | 6.3 |
| tui_partial parser deadline misses ↓ | 0 | 0 | 118 | 0 |
| Peak sampled RSS (MiB) ↓ | 191.5 | 132.6 | 152.1 | 133.7 |
| Peak sampled physical footprint (MiB) ↓ | 165.0 | 94.9 | 87.0 | 96.4 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- loomtty: `loomtty 0.2.1`; binary SHA-256 `26a47e68892414b2c4714ab0e1b4bfb19ea12b77266ecc24de5b3ed461048d66`
  - loomtty-server SHA-256 `5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`
