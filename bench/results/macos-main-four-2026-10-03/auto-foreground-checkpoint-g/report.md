# macOS terminal benchmark

Generated: 2026-10-03T13:28:22.782596+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | — | — | 1 | — |
| Launch → first DSR response (ms) ↓ | — | — | 1060.4 (1060.4–1060.4) | — |
| Cold idle RSS (MiB) ↓ | — | — | 125.6 | — |
| Cold idle physical footprint (MiB) ↓ | — | — | 60.9 | — |
| Cold idle CPU (% of one core) ↓ | — | — | 0.03 | — |
| Warm idle RSS (MiB) ↓ | — | — | 134.2 | — |
| Warm idle physical footprint (MiB) ↓ | — | — | 63.4 | — |
| Warm idle CPU (% of one core) ↓ | — | — | 0.01 | — |
| After history RSS (MiB) ↓ | — | — | 168.3 | — |
| After history physical footprint (MiB) ↓ | — | — | 97.4 | — |
| After history CPU (% of one core) ↓ | — | — | 0.01 | — |
| ascii parser throughput (MiB/s) ↑ | — | — | 214.3 (214.3–214.3) | — |
| ascii terminal CPU (s) ↓ | — | — | 0.034 | — |
| truecolor parser throughput (MiB/s) ↑ | — | — | 171.0 (171.0–171.0) | — |
| truecolor terminal CPU (s) ↓ | — | — | 0.044 | — |
| cjk parser throughput (MiB/s) ↑ | — | — | 177.0 (177.0–177.0) | — |
| cjk terminal CPU (s) ↓ | — | — | 0.041 | — |
| emoji parser throughput (MiB/s) ↑ | — | — | 189.7 (189.7–189.7) | — |
| emoji terminal CPU (s) ↓ | — | — | 0.038 | — |
| scroll parser throughput (MiB/s) ↑ | — | — | 62.0 (62.0–62.0) | — |
| scroll terminal CPU (s) ↓ | — | — | 0.125 | — |
| cursor parser throughput (MiB/s) ↑ | — | — | 189.2 (189.2–189.2) | — |
| cursor terminal CPU (s) ↓ | — | — | 0.039 | — |
| latency DSR p50 (ms) ↓ | — | — | 3.527 | — |
| latency DSR p95 (ms) ↓ | — | — | 3.585 | — |
| latency DSR p99 (ms) ↓ | — | — | 3.647 | — |
| loaded_latency DSR p50 (ms) ↓ | — | — | 3.653 | — |
| loaded_latency DSR p95 (ms) ↓ | — | — | 3.864 | — |
| loaded_latency DSR p99 (ms) ↓ | — | — | 3.919 | — |
| tui_full DSR p50 (ms) ↓ | — | — | 3.283 | — |
| tui_full DSR p95 (ms) ↓ | — | — | 4.809 | — |
| tui_full DSR p99 (ms) ↓ | — | — | 6.682 | — |
| tui_full terminal CPU (%) ↓ | — | — | 12.3 | — |
| tui_full parser deadline misses ↓ | — | — | 1 | — |
| tui_partial DSR p50 (ms) ↓ | — | — | 3.452 | — |
| tui_partial DSR p95 (ms) ↓ | — | — | 3.681 | — |
| tui_partial DSR p99 (ms) ↓ | — | — | 4.019 | — |
| tui_partial terminal CPU (%) ↓ | — | — | 9.0 | — |
| tui_partial parser deadline misses ↓ | — | — | 0 | — |
| vtebench cursor_motion PTY throughput (MiB/s) ↑ | — | — | 160.5 (160.5–160.5) | — |
| vtebench cursor_motion CPU ms/MiB ↓ | — | — | 6.755 | — |
| vtebench cursor_motion physical footprint (MiB) ↓ | — | — | 93.3 | — |
| vtebench dense_cells PTY throughput (MiB/s) ↑ | — | — | 187.8 (187.8–187.8) | — |
| vtebench dense_cells CPU ms/MiB ↓ | — | — | 5.466 | — |
| vtebench dense_cells physical footprint (MiB) ↓ | — | — | 93.5 | — |
| vtebench light_cells PTY throughput (MiB/s) ↑ | — | — | 242.7 (242.7–242.7) | — |
| vtebench light_cells CPU ms/MiB ↓ | — | — | 3.738 | — |
| vtebench light_cells physical footprint (MiB) ↓ | — | — | 93.5 | — |
| vtebench medium_cells PTY throughput (MiB/s) ↑ | — | — | 151.0 (151.0–151.0) | — |
| vtebench medium_cells CPU ms/MiB ↓ | — | — | 7.155 | — |
| vtebench medium_cells physical footprint (MiB) ↓ | — | — | 94.2 | — |
| vtebench scrolling PTY throughput (MiB/s) ↑ | — | — | 16.9 (16.9–16.9) | — |
| vtebench scrolling CPU ms/MiB ↓ | — | — | 61.295 | — |
| vtebench scrolling physical footprint (MiB) ↓ | — | — | 94.3 | — |
| vtebench scrolling_bottom_region PTY throughput (MiB/s) ↑ | — | — | 42.8 (42.8–42.8) | — |
| vtebench scrolling_bottom_region CPU ms/MiB ↓ | — | — | 24.575 | — |
| vtebench scrolling_bottom_region physical footprint (MiB) ↓ | — | — | 94.3 | — |
| vtebench scrolling_bottom_small_region PTY throughput (MiB/s) ↑ | — | — | 42.9 (42.9–42.9) | — |
| vtebench scrolling_bottom_small_region CPU ms/MiB ↓ | — | — | 24.646 | — |
| vtebench scrolling_bottom_small_region physical footprint (MiB) ↓ | — | — | 95.2 | — |
| vtebench scrolling_fullscreen PTY throughput (MiB/s) ↑ | — | — | 168.4 (168.4–168.4) | — |
| vtebench scrolling_fullscreen CPU ms/MiB ↓ | — | — | 5.970 | — |
| vtebench scrolling_fullscreen physical footprint (MiB) ↓ | — | — | 95.2 | — |
| vtebench scrolling_top_region PTY throughput (MiB/s) ↑ | — | — | 42.4 (42.4–42.4) | — |
| vtebench scrolling_top_region CPU ms/MiB ↓ | — | — | 24.761 | — |
| vtebench scrolling_top_region physical footprint (MiB) ↓ | — | — | 95.2 | — |
| vtebench scrolling_top_small_region PTY throughput (MiB/s) ↑ | — | — | 43.1 (43.1–43.1) | — |
| vtebench scrolling_top_small_region CPU ms/MiB ↓ | — | — | 24.371 | — |
| vtebench scrolling_top_small_region physical footprint (MiB) ↓ | — | — | 95.2 | — |
| vtebench sync_medium_cells PTY throughput (MiB/s) ↑ | — | — | 90.6 (90.6–90.6) | — |
| vtebench sync_medium_cells CPU ms/MiB ↓ | — | — | 11.590 | — |
| vtebench sync_medium_cells physical footprint (MiB) ↓ | — | — | 95.2 | — |
| vtebench unicode PTY throughput (MiB/s) ↑ | — | — | 133.0 (133.0–133.0) | — |
| vtebench unicode CPU ms/MiB ↓ | — | — | 9.087 | — |
| vtebench unicode physical footprint (MiB) ↓ | — | — | 616.7 | — |
| Peak sampled RSS (MiB) ↓ | — | — | 714.7 | — |
| Peak sampled physical footprint (MiB) ↓ | — | — | 616.8 | — |

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

## Failed runs (not included in medians)

- alacritty round 0: concurrent Rust build/test processes detected: 60649; retry when idle
