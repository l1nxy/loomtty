# macOS terminal benchmark

Generated: 2026-10-02T03:39:16.677778+00:00

Apple M6; macOS 27.0; 100×30 cells; Menlo 10 pt.

Cells below are medians across fresh launches. Ranges are min–max, not confidence intervals.

| Metric | loomtty | alacritty | kitty | ghostty |
|---|---:|---:|---:|---:|
| Successful launches | — | — | — | — |
| Launch → first DSR response (ms) ↓ | — | — | — | — |
| Cold idle RSS (MiB) ↓ | — | — | — | — |
| Cold idle physical footprint (MiB) ↓ | — | — | — | — |
| Cold idle CPU (% of one core) ↓ | — | — | — | — |
| Warm idle RSS (MiB) ↓ | — | — | — | — |
| Warm idle physical footprint (MiB) ↓ | — | — | — | — |
| Warm idle CPU (% of one core) ↓ | — | — | — | — |
| After history RSS (MiB) ↓ | — | — | — | — |
| After history physical footprint (MiB) ↓ | — | — | — | — |
| After history CPU (% of one core) ↓ | — | — | — | — |
| ascii parser throughput (MiB/s) ↑ | — | — | — | — |
| ascii terminal CPU (s) ↓ | — | — | — | — |
| truecolor parser throughput (MiB/s) ↑ | — | — | — | — |
| truecolor terminal CPU (s) ↓ | — | — | — | — |
| cjk parser throughput (MiB/s) ↑ | — | — | — | — |
| cjk terminal CPU (s) ↓ | — | — | — | — |
| emoji parser throughput (MiB/s) ↑ | — | — | — | — |
| emoji terminal CPU (s) ↓ | — | — | — | — |
| scroll parser throughput (MiB/s) ↑ | — | — | — | — |
| scroll terminal CPU (s) ↓ | — | — | — | — |
| cursor parser throughput (MiB/s) ↑ | — | — | — | — |
| cursor terminal CPU (s) ↓ | — | — | — | — |
| latency DSR p50 (ms) ↓ | — | — | — | — |
| latency DSR p95 (ms) ↓ | — | — | — | — |
| latency DSR p99 (ms) ↓ | — | — | — | — |
| loaded_latency DSR p50 (ms) ↓ | — | — | — | — |
| loaded_latency DSR p95 (ms) ↓ | — | — | — | — |
| loaded_latency DSR p99 (ms) ↓ | — | — | — | — |
| tui_full DSR p50 (ms) ↓ | — | — | — | — |
| tui_full DSR p95 (ms) ↓ | — | — | — | — |
| tui_full DSR p99 (ms) ↓ | — | — | — | — |
| tui_full terminal CPU (%) ↓ | — | — | — | — |
| tui_full parser deadline misses ↓ | — | — | — | — |
| tui_partial DSR p50 (ms) ↓ | — | — | — | — |
| tui_partial DSR p95 (ms) ↓ | — | — | — | — |
| tui_partial DSR p99 (ms) ↓ | — | — | — | — |
| tui_partial terminal CPU (%) ↓ | — | — | — | — |
| tui_partial parser deadline misses ↓ | — | — | — | — |
| Peak sampled RSS (MiB) ↓ | — | — | — | — |
| Peak sampled physical footprint (MiB) ↓ | — | — | — | — |

## Scope and limitations

- loomtty includes both its GUI client and private server; other terminals include their GUI process. Workload Python/shell processes and WindowServer are excluded.
- DSR measures PTY delivery and terminal parsing. In loomtty the server answers it; it does not fence client rendering. These numbers are **not FPS or input-to-photon latency**.
- TUI workloads issue and acknowledge updates at 60 Hz. CPU includes a 250 ms settling tail. Deadline misses refer to parser acknowledgements, not displayed frames.
- Bulk tests run in the alternate screen so differing history storage cannot determine their result. The separate 10,000-line history test uses native limits: 10,000 lines vs Ghostty's 10 MB cap.
- Menlo and grid size match; rasterizer metrics, fallback fonts, chrome and renderer internals differ. A larger RSS is not necessarily a larger physical footprint; shared pages may be counted twice when summing loomtty processes.
- Fresh processes, warm OS/file caches; not a cold-boot startup benchmark. App order is deterministically shuffled per round. No disk cache flushing or system-wide tuning.
- Keep windows visible, connected to power and the machine otherwise idle. Window occlusion, background workloads, thermal state and macOS scheduling can affect results.

## Versions

- loomtty: `loomtty 0.2.1`; binary SHA-256 `bd09c5936127a337f3bcfe6bcc87231d16488b7a3d0d8a9a18e8d21fdade81dd`
  - loomtty-server SHA-256 `e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`
