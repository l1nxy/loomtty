# macOS memory and startup profile

Measured on 2026-09-30 with macOS 27.0, Apple M6 (arm64), Rust 1.94.0,
release builds, and the Metal backend. Baseline: `fe438ab` on
`fix/macos-adaptation`, after integrating all six macOS fix commits.
Individual samples and measurement boundaries are in
[`macos-memory-results.json`](macos-memory-results.json).

## Results

Medians of three runs per version:

| Measurement | Before | After |
| --- | ---: | ---: |
| Client idle RSS | 1074.80 MiB | 95.23 MiB |
| Client peak RSS during output | 1079.53 MiB | 100.31 MiB |
| Client launch to beginning session connection | 258.74 ms | 136.79 ms |
| Client CPU time over 6 seconds of output | 1.19 s | 1.18 s |
| Standalone font workload peak RSS | 999.00 MiB | 21.66 MiB |
| Standalone font initialization | 108.83 ms | 24.00 ms |

The client measurements used Menlo at 10 pt, a 2x display scale, the same
configuration and window size, and three interleaved before/after launches.
Each launch connected to its own session on a private test server socket.
After settling, the workload emitted 480 screen refreshes at a requested
120 Hz, each with 24 rows of ANSI colors, ASCII, Chinese, and emoji. RSS was
sampled every 100 ms for six seconds. The idle CPU interval was two seconds;
its median was 0.01 CPU seconds before and 0.00 after at `ps` precision.
The test sessions and server were removed afterward.

These results demonstrate lower memory use and faster initialization.
CPU consumption during the fixed-rate output workload was similar; these
samples do not establish a throughput or frame-rate improvement. Startup
timing stops at the client's `connecting to session` log, before the server
handshake and first presentation. RSS is client-only and is affected by OS
caching; it is distinct from Activity Monitor's memory footprint metric.
The standalone benchmark excludes GPU/window/server allocations.

## Changes

- CoreText loads font files through URL-backed descriptors, preserving the
  selected TTC face identity without retaining independent heap copies in each
  shaper and rasterizer. The system emoji collection on this host is 183 MB.
- Font identity is checked against the file's PostScript name: CoreText can
  reorder reserved PingFang collection descriptors. When the identities do
  not match, the name-based system-font fallback and Regular variation from
  the macOS fix branch are retained, using mapped metadata instead of copied
  collection bytes. The locale-selected PingFang SC font and style-glyph
  remapping remain covered by regressions.
- The macOS font resolver extracts coverage through fontdb's temporary
  mappings. Raw font data is loaded only when explicitly requested.
- Color-font detection enumerates table tags instead of copying the entire
  `sbix` bitmap table. The UI shaper receives file paths on macOS, including
  when it inherits the terminal font.
- Blade allocates upload buffers on demand and grows them to fit the current
  glyph payload. The default 2048-square atlases previously reserved 20 MiB
  of upload buffers immediately. Upload offsets are aligned and every queued
  glyph is transferred in the current submission.
- Glyph uniforms grow with draw-batch count. Initial capacity for both layers
  is 8 KiB, down from 16 MiB at the default 32768-glyph limit. These numbers
  describe buffer capacity, not measured resident memory.
- GPU render-pass clears replace atlas-sized CPU zero uploads. Clear data
  cannot be overwritten by glyph uploads queued in the same submission.
  Shared buffers are changed only after the previous GPU fence completes.

## Reproduce the font workload

```sh
cargo build --release -p loom-render --example font_profile
/usr/bin/time -l target/release/examples/font_profile
```

Run the already-built binary three times for each revision. To measure the
baseline, copy the identical example into its checkout before building.
`init_ms` includes system font discovery, cmap extraction, and rasterizer
setup. `workload_ms` covers 1000 passes over a fixed ASCII/CJK/emoji string
using the glyph and shaping caches. Both versions report 16x31-pixel cells.
On macOS, `/usr/bin/time -l` reports maximum RSS in bytes; divide by 1048576
for MiB. Keep raw samples so warm-cache effects remain visible.

## Verification

`cargo test --workspace` passed after integration (1601 tests, 12 ignored
documentation examples), including 293 client, 73 rendering, and 19 GPU
library tests. Release
builds, `cargo fmt --check`, and `cargo clippy --workspace --all-targets`
completed; Clippy reports existing warnings.

The new regressions cover all Menlo TTC faces and Apple Color Emoji,
ASCII/CJK/color-emoji rasterization, ZWJ shaping, UI font inheritance,
locale-selected CJK font identity, and bold/italic prompt-arrow rasterization.
The GPU test runs with Metal API validation and reads back every atlas pixel
after initialization, a small upload, buffer growth, and a clear plus
odd-sized glyph uploads in one submission, for both R8 and RGBA atlases.
GPU tests skip explicitly on hosts without a supported GPU. Vulkan shares
the Blade changes; runtime GPU validation for this work was on Metal.
