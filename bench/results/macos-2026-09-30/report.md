# macOS terminal benchmark

Generated: 2026-09-29T16:24:37.249712+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | — | 3 | 3 | 3 |
| Launch → first DSR response (ms) ↓ | — | 812.6 (559.7–821.9) | 923.9 (586.3–1011.7) | 545.2 (528.8–852.4) |
| Cold idle RSS (MiB) ↓ | — | 101.4 | 123.4 | 116.5 |
| Cold idle physical footprint (MiB) ↓ | — | 46.5 | 64.3 | 74.4 |
| Cold idle CPU (% of one core) ↓ | — | 0.13 | 0.04 | 0.64 |
| Warm idle RSS (MiB) ↓ | — | 107.9 | 132.3 | 125.0 |
| Warm idle physical footprint (MiB) ↓ | — | 51.6 | 66.7 | 86.6 |
| Warm idle CPU (% of one core) ↓ | — | 0.05 | 0.02 | 0.72 |
| After history RSS (MiB) ↓ | — | 135.2 | 167.4 | 134.2 |
| After history physical footprint (MiB) ↓ | — | 78.2 | 103.5 | 95.3 |
| After history CPU (% of one core) ↓ | — | 0.08 | 0.01 | 0.74 |
| ascii parser throughput (MiB/s) ↑ | — | 117.7 (112.9–118.2) | 108.3 (107.5–109.6) | 79.1 (75.9–80.4) |
| ascii terminal CPU (s) ↓ | — | 0.081 | 0.083 | 0.154 |
| truecolor parser throughput (MiB/s) ↑ | — | 110.0 (107.6–113.4) | 107.0 (102.0–109.0) | 80.1 (79.9–80.1) |
| truecolor terminal CPU (s) ↓ | — | 0.081 | 0.096 | 0.156 |
| cjk parser throughput (MiB/s) ↑ | — | 110.1 (108.2–110.3) | 110.7 (110.1–133.1) | 86.6 (82.8–87.1) |
| cjk terminal CPU (s) ↓ | — | 0.084 | 0.089 | 0.152 |
| emoji parser throughput (MiB/s) ↑ | — | 108.8 (108.3–109.4) | 107.2 (106.5–115.9) | 91.4 (71.1–92.7) |
| emoji terminal CPU (s) ↓ | — | 0.084 | 0.084 | 0.152 |
| scroll parser throughput (MiB/s) ↑ | — | 103.9 (99.3–109.4) | 48.2 (47.5–48.4) | 67.4 (65.1–68.5) |
| scroll terminal CPU (s) ↓ | — | 0.083 | 0.173 | 0.147 |
| cursor parser throughput (MiB/s) ↑ | — | 109.2 (108.8–111.6) | 108.0 (107.6–108.1) | 79.7 (76.2–90.5) |
| cursor terminal CPU (s) ↓ | — | 0.081 | 0.086 | 0.153 |
| latency DSR p50 (ms) ↓ | — | 0.143 | 3.527 | 0.116 |
| latency DSR p95 (ms) ↓ | — | 1.097 | 3.577 | 0.160 |
| latency DSR p99 (ms) ↓ | — | 3.680 | 3.602 | 0.226 |
| loaded_latency DSR p50 (ms) ↓ | — | 0.303 | 3.652 | 0.379 |
| loaded_latency DSR p95 (ms) ↓ | — | 0.563 | 3.880 | 0.828 |
| loaded_latency DSR p99 (ms) ↓ | — | 0.768 | 3.989 | 0.985 |
| tui_full DSR p50 (ms) ↓ | — | 0.302 | 3.282 | 0.418 |
| tui_full DSR p95 (ms) ↓ | — | 0.387 | 3.379 | 0.636 |
| tui_full DSR p99 (ms) ↓ | — | 0.844 | 5.199 | 0.653 |
| tui_full terminal CPU (%) ↓ | — | 10.5 | 14.7 | 16.0 |
| tui_full parser deadline misses ↓ | — | 0 | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | — | 0.210 | 3.571 | 0.218 |
| tui_partial DSR p95 (ms) ↓ | — | 0.269 | 6.066 | 0.286 |
| tui_partial DSR p99 (ms) ↓ | — | 4.472 | 10.599 | 0.429 |
| tui_partial terminal CPU (%) ↓ | — | 10.9 | 5.2 | 7.5 |
| tui_partial parser deadline misses ↓ | — | 0 | 0 | 0 |
| Peak sampled RSS (MiB) ↓ | — | 135.2 | 167.5 | 134.2 |
| Peak sampled physical footprint (MiB) ↓ | — | 103.0 | 130.0 | 95.3 |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- loomtty: `loomtty 0.1.0`; binary SHA-256 `c5e7470c783b76b8f4cdca8300d1429424a54cfc6ba30be826310281fbed154f`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`

## Failed runs (not included in medians)

- loomtty round 0: Post-run audit: config watcher thread and cleanup panicked; excluded from valid medians
- loomtty round 1: Post-run audit: config watcher thread and cleanup panicked; excluded from valid medians
- loomtty round 2: Post-run audit: config watcher thread and cleanup panicked; excluded from valid medians
