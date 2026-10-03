# macOS terminal benchmark

Generated: 2026-10-03T13:10:26.767417+00:00

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

- Upstream vtebench: `ead80032e57dee2e75f0b51f2ea67528647d9944`; binary SHA-256 `901e9ec14c457ade9afb265b6fc31f9b9139cfd1c99a9f86cb095ea463a3fb70`
- vtebench is unmodified; BSD ps tty padding is trimmed only while generating payloads. All payload/setup bytes are hashed and frozen before measurement.
- Official timing measures blocked PTY writes with millisecond truncation, not parser completion or displayed FPS. It uses 16 MiB minimum samples, one warmup, and the configured time limit.
- CPU ms/MiB includes setup, warmup and drain/settling; its denominator counts payload bytes including warmup, excluding small setup/reset/status bytes.
- PTY output modes (mode, flags): []. Missing legacy metadata means raw output, but the exact flags were not recorded. Payload byte counts precede kernel output processing.
- loomtty: `loomtty 0.2.1`; binary SHA-256 `e467ffb2a2659a1a347461ae969cf5270393bb5091b2e1e55da7e1e59afc52af`
  - loomtty-server SHA-256 `2b69ee4c17b103718ff746e872a5ea77bb624712bc72a3885dd3cc851ff6c331`
- alacritty: `alacritty 0.17.0 (94e7c88)`; binary SHA-256 `1cd82fd2ecd925d588c4dfbd11a26e4e291fd3f0cf1e23dcfeae3a0abc6925c3`
- kitty: `kitty 0.49.1 created by Kovid Goyal`; binary SHA-256 `6dbd9db86866e7e4a699fc482a61ed24c10ab7b562469b1e062c400b0492e74b`
- ghostty: `Ghostty 1.3.1`; binary SHA-256 `3460be6d0c80504ffafe0dbb06f60cb1e8fb680a564a97a1aa5b95b48b8e30ac`
