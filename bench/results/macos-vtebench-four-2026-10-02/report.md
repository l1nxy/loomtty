# macOS terminal benchmark — raw PTY, one run per app

Official phases inherited raw output flags (OPOST off). Upstream vtebench itself does not change termios. Do not mix these throughput figures with inherited-output-mode runs.

Sequential standalone launches, not a randomized repeated campaign. Per-app metadata and timings are in their original results.json files.

Generated: 2026-10-02T05:29:36.132054+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | 1 | 1 | 1 | 1 |
| Launch → first DSR response (ms) ↓ | 1825.1 (1825.1–1825.1) | 610.7 (610.7–610.7) | 875.6 (875.6–875.6) | 642.4 (642.4–642.4) |
| Cold idle RSS (MiB) ↓ | 116.8 | 103.4 | 126.1 | 121.6 |
| Cold idle physical footprint (MiB) ↓ | 60.8 | 41.5 | 60.7 | 63.9 |
| Cold idle CPU (% of one core) ↓ | 0.07 | 0.04 | 0.38 | 0.62 |
| Warm idle RSS (MiB) ↓ | 123.7 | 110.2 | 134.6 | 130.0 |
| Warm idle physical footprint (MiB) ↓ | 68.2 | 46.3 | 63.2 | 82.4 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.02 | 0.01 | 0.71 |
| After history RSS (MiB) ↓ | 185.0 | 137.3 | 169.1 | 138.5 |
| After history physical footprint (MiB) ↓ | 129.5 | 72.3 | 97.5 | 90.8 |
| After history CPU (% of one core) ↓ | 0.01 | 0.02 | 0.01 | 0.58 |
| ascii parser throughput (MiB/s) ↑ | 257.5 (257.5–257.5) | 168.5 (168.5–168.5) | 212.7 (212.7–212.7) | 126.1 (126.1–126.1) |
| ascii terminal CPU (s) ↓ | 0.045 | 0.054 | 0.035 | 0.100 |
| truecolor parser throughput (MiB/s) ↑ | 311.3 (311.3–311.3) | 166.9 (166.9–166.9) | 171.8 (171.8–171.8) | 127.1 (127.1–127.1) |
| truecolor terminal CPU (s) ↓ | 0.040 | 0.049 | 0.045 | 0.096 |
| cjk parser throughput (MiB/s) ↑ | 306.9 (306.9–306.9) | 143.9 (143.9–143.9) | 175.2 (175.2–175.2) | 143.8 (143.8–143.8) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.057 | 0.041 | 0.095 |
| emoji parser throughput (MiB/s) ↑ | 344.7 (344.7–344.7) | 193.8 (193.8–193.8) | 192.0 (192.0–192.0) | 149.4 (149.4–149.4) |
| emoji terminal CPU (s) ↓ | 0.037 | 0.045 | 0.037 | 0.096 |
| scroll parser throughput (MiB/s) ↑ | 213.9 (213.9–213.9) | 195.6 (195.6–195.6) | 62.2 (62.2–62.2) | 96.4 (96.4–96.4) |
| scroll terminal CPU (s) ↓ | 0.051 | 0.042 | 0.125 | 0.102 |
| cursor parser throughput (MiB/s) ↑ | 256.0 (256.0–256.0) | 166.2 (166.2–166.2) | 189.5 (189.5–189.5) | 126.4 (126.4–126.4) |
| cursor terminal CPU (s) ↓ | 0.044 | 0.054 | 0.039 | 0.096 |
| latency DSR p50 (ms) ↓ | 0.089 | 0.058 | 3.513 | 0.102 |
| latency DSR p95 (ms) ↓ | 0.170 | 4.222 | 3.571 | 0.146 |
| latency DSR p99 (ms) ↓ | 0.361 | 5.096 | 3.609 | 0.186 |
| loaded_latency DSR p50 (ms) ↓ | 0.150 | 0.188 | 3.563 | 0.260 |
| loaded_latency DSR p95 (ms) ↓ | 0.198 | 0.221 | 3.842 | 0.301 |
| loaded_latency DSR p99 (ms) ↓ | 0.234 | 3.115 | 3.885 | 0.355 |
| tui_full DSR p50 (ms) ↓ | 0.385 | 0.237 | 3.292 | 0.380 |
| tui_full DSR p95 (ms) ↓ | 0.621 | 0.297 | 3.376 | 0.603 |
| tui_full DSR p99 (ms) ↓ | 0.714 | 2.802 | 3.465 | 0.674 |
| tui_full terminal CPU (%) ↓ | 13.0 | 9.1 | 18.1 | 14.7 |
| tui_full parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.177 | 0.152 | 3.450 | 0.156 |
| tui_partial DSR p95 (ms) ↓ | 0.241 | 0.196 | 3.677 | 0.216 |
| tui_partial DSR p99 (ms) ↓ | 0.278 | 0.231 | 3.745 | 0.324 |
| tui_partial terminal CPU (%) ↓ | 8.5 | 8.3 | 8.9 | 6.4 |
| tui_partial parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 330.8 (330.8–330.8) | 174.3 (174.3–174.3) | 160.5 (160.5–160.5) | 126.6 (126.6–126.6) |
| vtebench cursor_motion CPU ms/MiB ↓ | 4.564 | 6.362 | 6.152 | 12.735 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 101.9 | 80.8 | 93.4 | 83.9 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 321.4 (321.4–321.4) | 175.9 (175.9–175.9) | 187.8 (187.8–187.8) | 115.2 (115.2–115.2) |
| vtebench dense_cells CPU ms/MiB ↓ | 4.654 | 6.400 | 5.250 | 11.512 |
| vtebench dense_cells physical footprint (MiB) ↓ | 102.2 | 81.0 | 93.6 | 84.9 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 302.3 (302.3–302.3) | 148.3 (148.3–148.3) | 239.1 (239.1–239.1) | 128.7 (128.7–128.7) |
| vtebench light_cells CPU ms/MiB ↓ | 4.878 | 7.503 | 3.715 | 12.335 |
| vtebench light_cells physical footprint (MiB) ↓ | 102.1 | 79.1 | 93.7 | 85.1 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 235.9 (235.9–235.9) | 155.4 (155.4–155.4) | 153.9 (153.9–153.9) | 106.0 (106.0–106.0) |
| vtebench medium_cells CPU ms/MiB ↓ | 5.723 | 7.179 | 6.368 | 13.228 |
| vtebench medium_cells physical footprint (MiB) ↓ | 112.5 | 81.5 | 95.2 | 85.8 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 28.2 (28.2–28.2) | 36.6 (36.6–36.6) | 17.9 (17.9–17.9) | 82.9 (82.9–82.9) |
| vtebench scrolling CPU ms/MiB ↓ | 40.283 | 28.356 | 56.622 | 15.498 |
| vtebench scrolling physical footprint (MiB) ↓ | 120.2 | 81.6 | 95.2 | 85.8 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 43.8 (43.8–43.8) | 42.4 (42.4–42.4) | 50.6 (50.6–50.6) | 76.9 (76.9–76.9) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 24.842 | 23.777 | 19.585 | 15.966 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 118.2 | 79.6 | 95.7 | 87.3 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 32.5 (32.5–32.5) | 31.9 (31.9–31.9) | 51.1 (51.1–51.1) | 72.1 (72.1–72.1) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 33.098 | 31.637 | 19.483 | 16.464 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 120.2 | 81.6 | 95.7 | 87.3 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 144.2 (144.2–144.2) | 195.1 (195.1–195.1) | 140.4 (140.4–140.4) | 128.0 (128.0–128.0) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 9.210 | 5.797 | 6.953 | 11.976 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 120.2 | 81.6 | 95.8 | 86.2 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 30.3 (30.3–30.3) | 30.0 (30.0–30.0) | 50.5 (50.5–50.5) | 76.9 (76.9–76.9) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 35.492 | 33.608 | 19.625 | 15.535 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 120.3 | 81.6 | 95.8 | 85.2 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 38.9 (38.9–38.9) | 38.6 (38.6–38.6) | 51.1 (51.1–51.1) | 78.0 (78.0–78.0) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 27.835 | 26.159 | 19.393 | 15.581 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 118.2 | 81.6 | 95.8 | 85.2 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 217.8 (217.8–217.8) | 133.2 (133.2–133.2) | 90.6 (90.6–90.6) | 97.1 (97.1–97.1) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.127 | 7.478 | 10.703 | 14.274 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 120.4 | 77.8 | 95.8 | 85.6 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 287.3 (287.3–287.3) | 167.6 (167.6–167.6) | 129.2 (129.2–129.2) | 141.1 (141.1–141.1) |
| vtebench unicode CPU ms/MiB ↓ | 7.404 | 9.581 | 9.126 | 12.359 |
| vtebench unicode physical footprint (MiB) ↓ | 163.3 | 141.7 | 616.6 | 215.5 |
| Peak sampled RSS (MiB) ↓ | 265.4 | 236.1 | 709.5 | 322.0 |
| Peak sampled physical footprint (MiB) ↓ | 163.3 | 141.7 | 617.4 | 215.7 |

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
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e8c11a399a3f46e2970ed519fb8128c48e0f9146e5f70068a4662ec18f0ca97c`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`
