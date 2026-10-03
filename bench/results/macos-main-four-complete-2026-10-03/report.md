# macOS terminal benchmark

Generated: 2026-10-03T13:42:54.032080+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | 3 | 3 | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 1633.5 (1365.8–2321.4) | 786.5 (611.6–957.5) | 1060.4 (287.7–2990.6) | 1512.1 (878.8–2098.9) |
| Cold idle RSS (MiB) ↓ | 114.5 | 103.1 | 125.6 | 118.3 |
| Cold idle physical footprint (MiB) ↓ | 64.1 | 41.7 | 60.9 | 68.7 |
| Cold idle CPU (% of one core) ↓ | 0.01 | 0.05 | 0.03 | 0.75 |
| Warm idle RSS (MiB) ↓ | 121.7 | 110.0 | 134.2 | 126.9 |
| Warm idle physical footprint (MiB) ↓ | 69.8 | 46.6 | 63.4 | 83.7 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.03 | 0.01 | 0.73 |
| After history RSS (MiB) ↓ | 183.1 | 137.2 | 168.4 | 136.5 |
| After history physical footprint (MiB) ↓ | 131.4 | 72.9 | 97.4 | 90.9 |
| After history CPU (% of one core) ↓ | 0.01 | 0.02 | 0.01 | 0.77 |
| ascii parser throughput (MiB/s) ↑ | 251.2 (249.4–252.6) | 160.6 (156.8–166.7) | 208.5 (204.1–214.3) | 130.0 (129.5–131.4) |
| ascii terminal CPU (s) ↓ | 0.046 | 0.058 | 0.034 | 0.098 |
| truecolor parser throughput (MiB/s) ↑ | 302.6 (297.9–305.2) | 180.9 (153.3–184.2) | 168.3 (167.1–171.0) | 127.1 (126.5–129.3) |
| truecolor terminal CPU (s) ↓ | 0.041 | 0.050 | 0.045 | 0.100 |
| cjk parser throughput (MiB/s) ↑ | 303.9 (301.5–311.6) | 142.3 (140.8–143.1) | 176.2 (174.6–177.0) | 147.2 (144.4–153.1) |
| cjk terminal CPU (s) ↓ | 0.041 | 0.063 | 0.041 | 0.092 |
| emoji parser throughput (MiB/s) ↑ | 338.6 (325.4–343.1) | 192.3 (190.4–199.0) | 189.2 (187.8–189.7) | 153.7 (153.6–156.2) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.047 | 0.038 | 0.093 |
| scroll parser throughput (MiB/s) ↑ | 208.1 (207.9–214.2) | 194.3 (193.7–195.7) | 62.0 (62.0–62.0) | 101.1 (99.9–101.5) |
| scroll terminal CPU (s) ↓ | 0.052 | 0.044 | 0.125 | 0.098 |
| cursor parser throughput (MiB/s) ↑ | 253.3 (247.2–254.1) | 162.9 (160.7–182.2) | 188.4 (186.7–189.2) | 129.7 (129.0–130.2) |
| cursor terminal CPU (s) ↓ | 0.046 | 0.056 | 0.039 | 0.100 |
| latency DSR p50 (ms) ↓ | 0.110 | 0.159 | 3.527 | 0.126 |
| latency DSR p95 (ms) ↓ | 0.164 | 0.498 | 3.935 | 0.178 |
| latency DSR p99 (ms) ↓ | 0.240 | 1.446 | 3.983 | 0.330 |
| loaded_latency DSR p50 (ms) ↓ | 0.147 | 0.187 | 3.653 | 0.253 |
| loaded_latency DSR p95 (ms) ↓ | 0.199 | 0.228 | 4.007 | 0.329 |
| loaded_latency DSR p99 (ms) ↓ | 0.222 | 0.359 | 4.096 | 0.453 |
| tui_full DSR p50 (ms) ↓ | 0.426 | 0.324 | 3.283 | 0.549 |
| tui_full DSR p95 (ms) ↓ | 0.652 | 0.478 | 4.809 | 1.237 |
| tui_full DSR p99 (ms) ↓ | 0.883 | 4.451 | 6.502 | 1.633 |
| tui_full terminal CPU (%) ↓ | 14.4 | 12.4 | 12.3 | 16.1 |
| tui_full parser deadline misses ↓ | 0 | 0 | 1 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.211 | 0.229 | 3.801 | 0.232 |
| tui_partial DSR p95 (ms) ↓ | 0.288 | 0.303 | 4.106 | 0.363 |
| tui_partial DSR p99 (ms) ↓ | 0.357 | 0.411 | 5.011 | 0.494 |
| tui_partial terminal CPU (%) ↓ | 9.5 | 11.3 | 10.0 | 8.1 |
| tui_partial parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 238.4 (234.9–241.9) | 270.2 (265.7–294.7) | 157.4 (157.4–160.5) | 116.6 (115.4–116.6) |
| vtebench cursor_motion CPU ms/MiB ↓ | 6.421 | 3.789 | 6.755 | 15.341 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 103.5 | 79.9 | 93.3 | 85.4 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 269.5 (261.1–278.5) | 211.5 (154.7–228.9) | 183.6 (183.6–187.8) | 116.9 (116.9–117.7) |
| vtebench dense_cells CPU ms/MiB ↓ | 5.818 | 5.070 | 5.471 | 12.096 |
| vtebench dense_cells physical footprint (MiB) ↓ | 103.9 | 79.8 | 93.5 | 86.5 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 281.1 (281.1–296.7) | 143.0 (140.5–145.6) | 239.1 (239.1–242.7) | 128.2 (127.7–128.2) |
| vtebench light_cells CPU ms/MiB ↓ | 5.138 | 7.567 | 3.734 | 12.474 |
| vtebench light_cells physical footprint (MiB) ↓ | 103.9 | 80.1 | 93.6 | 85.2 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 234.2 (230.8–234.2) | 155.4 (152.4–156.9) | 149.6 (148.9–151.0) | 102.9 (102.3–102.9) |
| vtebench medium_cells CPU ms/MiB ↓ | 6.343 | 6.668 | 7.155 | 15.538 |
| vtebench medium_cells physical footprint (MiB) ↓ | 104.8 | 80.2 | 94.2 | 85.9 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 82.5 (82.1–86.5) | 76.9 (76.0–77.7) | 16.9 (16.8–16.9) | 60.2 (59.7–60.4) |
| vtebench scrolling CPU ms/MiB ↓ | 16.501 | 13.408 | 61.295 | 25.964 |
| vtebench scrolling physical footprint (MiB) ↓ | 120.2 | 80.2 | 94.3 | 86.1 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 96.4 (95.8–96.4) | 87.9 (87.4–88.9) | 42.4 (42.4–42.8) | 54.6 (54.4–54.6) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 14.385 | 11.541 | 24.742 | 28.039 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 120.2 | 80.8 | 94.3 | 86.1 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 55.6 (55.4–55.6) | 52.8 (52.8–53.0) | 42.7 (42.7–42.9) | 53.9 (53.6–54.1) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 22.618 | 19.156 | 24.657 | 28.268 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 118.2 | 81.5 | 94.3 | 86.1 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 172.1 (172.1–177.8) | 158.4 (139.1–168.4) | 168.4 (166.7–168.4) | 126.0 (125.0–127.0) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 8.203 | 7.127 | 6.009 | 12.791 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 120.3 | 80.2 | 95.1 | 86.1 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 48.6 (48.5–48.8) | 48.2 (47.9–48.5) | 42.3 (42.3–42.4) | 60.8 (60.4–61.1) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 25.475 | 21.145 | 24.836 | 20.657 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 120.3 | 80.0 | 95.1 | 86.1 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 76.2 (76.2–76.2) | 70.2 (70.2–71.1) | 42.8 (42.6–43.1) | 61.8 (61.8–62.0) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 17.249 | 14.457 | 24.740 | 21.400 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 120.3 | 80.3 | 95.1 | 86.1 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 214.9 (214.9–217.8) | 126.9 (126.9–126.9) | 90.6 (90.1–90.6) | 100.7 (100.1–100.7) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.674 | 7.712 | 11.590 | 13.776 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 118.6 | 80.4 | 95.2 | 86.3 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 292.6 (292.6–292.6) | 165.9 (165.9–167.6) | 139.9 (133.0–147.6) | 141.1 (137.5–141.1) |
| vtebench unicode CPU ms/MiB ↓ | 7.562 | 9.306 | 8.720 | 12.302 |
| vtebench unicode physical footprint (MiB) ↓ | 164.5 | 139.4 | 573.9 | 215.1 |
| Peak sampled RSS (MiB) ↓ | 262.9 | 234.7 | 672.6 | 317.2 |
| Peak sampled physical footprint (MiB) ↓ | 164.5 | 139.4 | 574.7 | 215.3 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- Upstream vtebench: `ead80032e57dee2e75f0b51f2ea67528647d9944`; binary SHA-256 `901e9ec14c457ade9afb265b6fc31f9b9139cfd1c99a9f86cb095ea463a3fb70`
- vtebench is unmodified; BSD ps tty padding is trimmed only while generating payloads. All payload/setup bytes are hashed and frozen before measurement.
- Official timing measures blocked PTY writes with millisecond truncation, not parser completion or displayed FPS. It uses 16 MiB minimum samples, one warmup, and the configured time limit.
- CPU ms/MiB includes setup, warmup and drain/settling; its denominator counts payload bytes including warmup, excluding small setup/reset/status bytes.
- PTY output modes (mode, flags): [('original', '3')]. Missing legacy metadata means raw output, but the exact flags were not recorded. Payload byte counts precede kernel output processing.
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e467ffb2a2659a1a347461ae969cf5270393bb5091b2e1e55da7e1e59afc52af`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`

## Supplemental runs

Failed initial launches were retried in separate batches. These are not three uninterrupted shuffled rounds. Each run's source_batch and source_directory, and source_batches metadata, preserve provenance; all failed attempts remain excluded from medians.
