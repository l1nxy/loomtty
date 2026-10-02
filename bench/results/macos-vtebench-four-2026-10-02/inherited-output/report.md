# macOS terminal benchmark

Generated: 2026-10-02T06:15:37.360923+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | 1 | 1 | 1 | 1 |
| Launch → first DSR response (ms) ↓ | 663.2 (663.2–663.2) | 432.2 (432.2–432.2) | 822.5 (822.5–822.5) | 614.4 (614.4–614.4) |
| Cold idle RSS (MiB) ↓ | 117.1 | 104.0 | 126.0 | 120.8 |
| Cold idle physical footprint (MiB) ↓ | 61.7 | 41.8 | 58.6 | 67.0 |
| Cold idle CPU (% of one core) ↓ | 0.00 | 0.04 | 0.00 | 0.77 |
| Warm idle RSS (MiB) ↓ | 124.1 | 110.8 | 134.5 | 129.4 |
| Warm idle physical footprint (MiB) ↓ | 68.2 | 46.6 | 64.2 | 85.5 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.02 | 0.01 | 0.70 |
| After history RSS (MiB) ↓ | 185.5 | 137.8 | 168.7 | 138.2 |
| After history physical footprint (MiB) ↓ | 129.9 | 72.5 | 98.1 | 92.2 |
| After history CPU (% of one core) ↓ | 0.01 | 0.02 | 0.01 | 0.72 |
| ascii parser throughput (MiB/s) ↑ | 244.6 (244.6–244.6) | 178.0 (178.0–178.0) | 210.2 (210.2–210.2) | 123.6 (123.6–123.6) |
| ascii terminal CPU (s) ↓ | 0.047 | 0.046 | 0.035 | 0.103 |
| truecolor parser throughput (MiB/s) ↑ | 304.8 (304.8–304.8) | 176.6 (176.6–176.6) | 171.5 (171.5–171.5) | 121.8 (121.8–121.8) |
| truecolor terminal CPU (s) ↓ | 0.040 | 0.049 | 0.044 | 0.102 |
| cjk parser throughput (MiB/s) ↑ | 312.4 (312.4–312.4) | 154.2 (154.2–154.2) | 175.5 (175.5–175.5) | 139.8 (139.8–139.8) |
| cjk terminal CPU (s) ↓ | 0.040 | 0.055 | 0.043 | 0.098 |
| emoji parser throughput (MiB/s) ↑ | 342.5 (342.5–342.5) | 189.5 (189.5–189.5) | 191.3 (191.3–191.3) | 150.8 (150.8–150.8) |
| emoji terminal CPU (s) ↓ | 0.038 | 0.045 | 0.037 | 0.096 |
| scroll parser throughput (MiB/s) ↑ | 213.3 (213.3–213.3) | 173.6 (173.6–173.6) | 62.4 (62.4–62.4) | 96.3 (96.3–96.3) |
| scroll terminal CPU (s) ↓ | 0.050 | 0.043 | 0.125 | 0.103 |
| cursor parser throughput (MiB/s) ↑ | 254.6 (254.6–254.6) | 198.0 (198.0–198.0) | 189.8 (189.8–189.8) | 123.8 (123.8–123.8) |
| cursor terminal CPU (s) ↓ | 0.045 | 0.042 | 0.039 | 0.105 |
| latency DSR p50 (ms) ↓ | 0.090 | 0.061 | 3.524 | 0.101 |
| latency DSR p95 (ms) ↓ | 0.156 | 4.525 | 3.604 | 0.133 |
| latency DSR p99 (ms) ↓ | 0.230 | 5.278 | 3.637 | 0.209 |
| loaded_latency DSR p50 (ms) ↓ | 0.141 | 0.194 | 3.369 | 0.264 |
| loaded_latency DSR p95 (ms) ↓ | 0.193 | 0.238 | 3.587 | 0.317 |
| loaded_latency DSR p99 (ms) ↓ | 0.226 | 3.682 | 3.668 | 0.402 |
| tui_full DSR p50 (ms) ↓ | 0.343 | 0.170 | 3.263 | 0.356 |
| tui_full DSR p95 (ms) ↓ | 0.634 | 1.071 | 3.374 | 0.573 |
| tui_full DSR p99 (ms) ↓ | 0.719 | 2.159 | 3.490 | 0.686 |
| tui_full terminal CPU (%) ↓ | 11.9 | 5.9 | 13.0 | 13.3 |
| tui_full parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.206 | 0.140 | 3.567 | 0.202 |
| tui_partial DSR p95 (ms) ↓ | 0.387 | 0.192 | 3.627 | 0.252 |
| tui_partial DSR p99 (ms) ↓ | 0.618 | 0.268 | 3.666 | 0.316 |
| tui_partial terminal CPU (%) ↓ | 7.9 | 6.6 | 9.0 | 7.0 |
| tui_partial parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | 234.9 (234.9–234.9) | 274.8 (274.8–274.8) | 154.4 (154.4–154.4) | 115.8 (115.8–115.8) |
| vtebench cursor_motion CPU ms/MiB ↓ | 6.528 | 3.685 | 6.824 | 15.319 |
| vtebench cursor_motion physical footprint (MiB) ↓ | 102.5 | 80.7 | 94.1 | 85.4 |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | 269.5 (269.5–269.5) | 179.7 (179.7–179.7) | 185.7 (185.7–185.7) | 110.7 (110.7–110.7) |
| vtebench dense_cells CPU ms/MiB ↓ | 5.752 | 5.779 | 5.506 | 12.442 |
| vtebench dense_cells physical footprint (MiB) ↓ | 102.8 | 80.5 | 94.2 | 86.5 |
| vtebench light_cells PTY throughput (MiB/s) ↑ | 296.7 (296.7–296.7) | 135.8 (135.8–135.8) | 242.7 (242.7–242.7) | 126.2 (126.2–126.2) |
| vtebench light_cells CPU ms/MiB ↓ | 4.944 | 7.790 | 3.721 | 12.378 |
| vtebench light_cells physical footprint (MiB) ↓ | 102.8 | 82.7 | 94.3 | 86.5 |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | 230.8 (230.8–230.8) | 141.7 (141.7–141.7) | 146.9 (146.9–146.9) | 102.3 (102.3–102.3) |
| vtebench medium_cells CPU ms/MiB ↓ | 6.370 | 7.574 | 7.228 | 15.644 |
| vtebench medium_cells physical footprint (MiB) ↓ | 111.2 | 80.7 | 95.0 | 87.4 |
| vtebench scrolling PTY throughput (MiB/s) ↑ | 82.1 (82.1–82.1) | 76.2 (76.2–76.2) | 16.8 (16.8–16.8) | 59.0 (59.0–59.0) |
| vtebench scrolling CPU ms/MiB ↓ | 16.704 | 13.411 | 61.642 | 26.735 |
| vtebench scrolling physical footprint (MiB) ↓ | 120.1 | 82.7 | 95.8 | 87.6 |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | 95.2 (95.2–95.2) | 87.4 (87.4–87.4) | 42.3 (42.3–42.3) | 48.6 (48.6–48.6) |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | 14.657 | 11.529 | 24.823 | 31.311 |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | 120.1 | 80.7 | 95.8 | 87.6 |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | 56.3 (56.3–56.3) | 53.1 (53.1–53.1) | 42.6 (42.6–42.6) | 53.2 (53.2–53.2) |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | 22.747 | 18.960 | 24.784 | 28.933 |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | 120.1 | 82.7 | 95.8 | 87.6 |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | 173.9 (173.9–173.9) | 179.8 (179.8–179.8) | 166.7 (166.7–166.7) | 127.0 (127.0–127.0) |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | 8.145 | 5.970 | 6.101 | 12.381 |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | 120.7 | 82.8 | 95.9 | 87.6 |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | 49.4 (49.4–49.4) | 47.3 (47.3–47.3) | 41.9 (41.9–41.9) | 60.3 (60.3–60.3) |
| vtebench scrolling_top_region CPU ms/MiB ↓ | 25.456 | 21.268 | 25.109 | 21.046 |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | 118.7 | 81.3 | 95.9 | 87.6 |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | 75.5 (75.5–75.5) | 70.8 (70.8–70.8) | 42.8 (42.8–42.8) | 61.3 (61.3–61.3) |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | 18.331 | 14.260 | 24.715 | 21.552 |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | 120.7 | 81.3 | 95.9 | 87.6 |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | 214.9 (214.9–214.9) | 119.4 (119.4–119.4) | 90.1 (90.1–90.1) | 97.7 (97.7–97.7) |
| vtebench sync_medium_cells CPU ms/MiB ↓ | 6.632 | 8.555 | 11.629 | 14.125 |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | 128.8 | 79.3 | 95.9 | 87.8 |
| vtebench unicode PTY throughput (MiB/s) ↑ | 300.8 (300.8–300.8) | 169.4 (169.4–169.4) | 134.1 (134.1–134.1) | 138.7 (138.7–138.7) |
| vtebench unicode CPU ms/MiB ↓ | 6.983 | 9.469 | 9.031 | 12.378 |
| vtebench unicode physical footprint (MiB) ↓ | 163.4 | 147.2 | 621.3 | 216.3 |
| Peak sampled RSS (MiB) ↓ | 265.3 | 236.8 | 714.1 | 318.5 |
| Peak sampled physical footprint (MiB) ↓ | 163.4 | 147.2 | 621.9 | 216.5 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. Each app was measured once in separate sequential batches; Loomtty was measured earlier. This is not a repeated shuffled cohort. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- Upstream vtebench: `ead80032e57dee2e75f0b51f2ea67528647d9944`; binary SHA-256 `901e9ec14c457ade9afb265b6fc31f9b9139cfd1c99a9f86cb095ea463a3fb70`
- vtebench is unmodified; BSD ps tty padding is trimmed only while generating payloads. All payload/setup bytes are hashed and frozen before measurement.
- Official timing measures blocked PTY writes with millisecond truncation, not parser completion or displayed FPS. It uses 16 MiB minimum samples, one warmup, and the configured time limit.
- CPU ms/MiB includes setup, warmup and drain/settling; its denominator counts payload bytes including warmup, excluding small setup/reset/status bytes.
- PTY output modes (mode, flags): [('original', '3')]. Missing legacy metadata means raw output, but the exact flags were not recorded. Payload byte counts precede kernel output processing.
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e8c11a399a3f46e2970ed519fb8128c48e0f9146e5f70068a4662ec18f0ca97c`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`

Foreground: Kitty manually confirmed; Alacritty/Ghostty used automatic foreground on launch per user instruction. Loomtty had CUA verification. No continuous occlusion monitoring.
