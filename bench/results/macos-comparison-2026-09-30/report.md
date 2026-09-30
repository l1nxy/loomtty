# macOS terminal benchmark

Generated: 2026-09-29T17:13:26.926411+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | 3 | 3 | 3 | 3 |
| Launch → first DSR response (ms) ↓ | 814.5 (528.7–1008.9) | 881.8 (876.5–935.1) | 985.1 (953.1–1025.8) | 839.4 (776.7–866.7) |
| Cold idle RSS (MiB) ↓ | 100.9 | 97.2 | 116.5 | 112.5 |
| Cold idle physical footprint (MiB) ↓ | 56.8 | 33.0 | 46.2 | 38.5 |
| Cold idle CPU (% of one core) ↓ | 0.03 | 0.05 | 0.03 | 0.06 |
| Warm idle RSS (MiB) ↓ | 107.8 | 104.0 | 116.5 | 122.1 |
| Warm idle physical footprint (MiB) ↓ | 57.5 | 38.7 | 42.9 | 55.9 |
| Warm idle CPU (% of one core) ↓ | 0.01 | 0.10 | 0.06 | 0.16 |
| After history RSS (MiB) ↓ | 153.6 | 131.1 | 150.6 | 131.2 |
| After history physical footprint (MiB) ↓ | 101.3 | 62.2 | 76.7 | 64.5 |
| After history CPU (% of one core) ↓ | 0.02 | 0.05 | 0.01 | 0.04 |
| ascii parser throughput (MiB/s) ↑ | 217.5 (212.8–218.9) | 199.3 (124.1–200.9) | 208.9 (206.5–210.4) | 123.6 (117.5–124.4) |
| ascii terminal CPU (s) ↓ | 0.052 | 0.049 | 0.031 | 0.134 |
| truecolor parser throughput (MiB/s) ↑ | 257.8 (256.4–267.7) | 176.7 (176.6–176.7) | 96.2 (95.5–98.1) | 119.7 (114.7–121.3) |
| truecolor terminal CPU (s) ↓ | 0.044 | 0.052 | 0.078 | 0.135 |
| cjk parser throughput (MiB/s) ↑ | 268.4 (265.7–273.9) | 181.7 (178.4–182.3) | 176.0 (175.4–176.0) | 121.0 (120.5–127.8) |
| cjk terminal CPU (s) ↓ | 0.043 | 0.052 | 0.039 | 0.128 |
| emoji parser throughput (MiB/s) ↑ | 294.6 (293.5–299.7) | 190.2 (187.2–191.1) | 188.5 (188.1–188.9) | 124.6 (120.1–127.1) |
| emoji terminal CPU (s) ↓ | 0.041 | 0.048 | 0.035 | 0.124 |
| scroll parser throughput (MiB/s) ↑ | 185.1 (184.4–186.4) | 201.7 (201.1–204.6) | 62.4 (62.4–62.5) | 67.7 (65.0–76.7) |
| scroll terminal CPU (s) ↓ | 0.057 | 0.042 | 0.121 | 0.227 |
| cursor parser throughput (MiB/s) ↑ | 213.4 (212.5–218.2) | 199.8 (197.4–202.0) | 165.6 (162.5–168.1) | 121.4 (119.6–123.0) |
| cursor terminal CPU (s) ↓ | 0.051 | 0.046 | 0.042 | 0.135 |
| latency DSR p50 (ms) ↓ | 0.090 | 0.125 | 3.182 | 0.108 |
| latency DSR p95 (ms) ↓ | 0.128 | 0.173 | 3.896 | 0.140 |
| latency DSR p99 (ms) ↓ | 0.158 | 0.525 | 3.964 | 0.180 |
| loaded_latency DSR p50 (ms) ↓ | 0.150 | 0.191 | 4.557 | 0.349 |
| loaded_latency DSR p95 (ms) ↓ | 0.185 | 0.226 | 5.245 | 0.530 |
| loaded_latency DSR p99 (ms) ↓ | 0.210 | 0.388 | 5.288 | 0.552 |
| tui_full DSR p50 (ms) ↓ | 0.477 | 0.227 | 3.226 | 0.341 |
| tui_full DSR p95 (ms) ↓ | 0.756 | 0.269 | 3.282 | 0.546 |
| tui_full DSR p99 (ms) ↓ | 0.810 | 0.317 | 3.308 | 0.580 |
| tui_full terminal CPU (%) ↓ | 14.9 | 9.2 | 2.8 | 15.0 |
| tui_full parser deadline misses ↓ | 0 | 0 | 0 | 0 |
| tui_partial DSR p50 (ms) ↓ | 0.136 | 0.156 | 3.944 | 0.133 |
| tui_partial DSR p95 (ms) ↓ | 0.184 | 0.215 | 190.086 | 0.183 |
| tui_partial DSR p99 (ms) ↓ | 0.225 | 1.045 | 191.248 | 0.326 |
| tui_partial terminal CPU (%) ↓ | 5.9 | 8.2 | 0.3 | 6.8 |
| tui_partial parser deadline misses ↓ | 0 | 0 | 141 | 0 |
| Peak sampled RSS (MiB) ↓ | 153.7 | 131.2 | 150.6 | 131.2 |
| Peak sampled physical footprint (MiB) ↓ | 121.6 | 88.6 | 76.7 | 85.0 |

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
  - loomtty-server SHA-256 `74cc781612c2c61e134d94fca24b60124affed7d168adfc19b8c740f1c7d8737`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`

## Supplemental runs

Failed initial launches were retried in separate batches. These are not three uninterrupted shuffled rounds. Each run's source_batch and source_directory, and source_batches metadata, preserve provenance; all failed attempts remain excluded from medians.

## Failed runs (not included in medians)

- ghostty round 0: timed out waiting for /private/tmp/loom-four-way-final-20260930/r0-ghostty/ready.json
- ghostty round 1: timed out waiting for /private/tmp/loom-four-way-final-20260930/r1-ghostty/ready.json
- kitty round 1: concurrent Rust build/test processes detected: 65736, 65771, 65776, 65778; retry when idle
- ghostty round 2: timed out waiting for /private/tmp/loom-four-way-final-20260930/r2-ghostty/ready.json
- kitty round 1: concurrent Rust build/test processes detected: 66274; retry when idle
- ghostty round 0: concurrent Rust build/test processes detected: 67812, 67849, 67853, 67854; retry when idle
- ghostty round 1: concurrent Rust build/test processes detected: 67812, 67892, 67897, 67898, 67899, 67904, 67907, 67909, 67913, 67915, 67916, 67917; retry when idle
- ghostty round 2: concurrent Rust build/test processes detected: 67812, 67897, 67898, 67904, 67907, 67909, 67916, 67921, 67923, 67930, 67931, 67934, 67937; retry when idle
- ghostty round 0: concurrent Rust build/test processes detected: 68315, 68316; retry when idle
