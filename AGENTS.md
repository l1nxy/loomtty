# loom — Agent Guide

`loom` (client binary: `loomtty`) is a GPU-accelerated terminal multiplexer
written in Rust, with a tmux/zellij-style client–server split. This guide helps
coding agents and contributors build, run, and navigate the workspace.

See also: [`README.md`](README.md) for user-facing CLI / keybindings / config,
[`INSTALL.md`](INSTALL.md) for a step-by-step build & install walkthrough, and
[`llms.txt`](llms.txt) for a condensed architecture index.

## 1. Build / Run / Test

All commands run from the workspace root. Rust edition 2024 (stable toolchain).

### Build
- Whole workspace (debug): `cargo build`
- Optimized binaries: `cargo build --release`

### Run
There are two binaries:
- `loomtty` — GUI client (winit event loop + GPU renderer); package `loomtty`
  (`crates/loom`).
- `loomtty-server` — background daemon; package `loomtty-server`
  (`crates/loom-server`).

Typical workflows:
- Client: `cargo run -p loomtty -- [client-args]`
- Server: `cargo run -p loomtty-server -- [server-args]`

The client autostarts the server on demand, so you rarely launch the daemon by
hand. On Unix the server listens on a Unix domain socket; on Windows it uses a
loopback TCP socket. Client and server communicate over the binary
`loom-protocol`.

### Test
- All: `cargo test`
- One crate: `cargo test -p loom-input`
- One test: `cargo test -p loom-input <test_name_or_path>`
- Cross-crate integration tests live in `crates/loom-integration-tests`.

### Lint / Format
- `cargo clippy --workspace --all-targets`
- `cargo fmt`

## 2. Workspace layout

A 17-crate workspace: two binaries plus fifteen libraries. The crate graph is a
DAG (no cycles).

### Binaries
- **`crates/loom`** (bin `loomtty`) — GUI client. Owns the winit event loop and
  GPU presentation, and wires together input (`loom-input`), layout
  (`loom-layout`), animation (`loom-anim` / `loom-motion`), chrome UI
  (`loom-ui`), rendering (`loom-render` + `loom-gpu`), config (`loom-config`),
  and client session flow (`loom-session`). Platform-agnostic client logic lives
  in `loom-app`.
- **`crates/loom-server`** (bin `loomtty-server`) — tokio daemon. Manages PTYs
  via `loom-term`, holds server-side layout/sessions, runs the Lua plugin engine
  (`loom-plugin`) and the web gateway, and encodes/decodes `loom-protocol`.
  Ships shell-integration scripts at `shell-integration/loom.{bash,zsh,fish}`.

### Libraries
- **`loom-app`** — platform-agnostic client core: `AppModel` state, pane grids,
  predictive echo, paste guard. No windowing/GPU deps, so it stays unit-testable.
- **`loom-ui`** — declarative, gpui-inspired UI toolkit (a Tailwind-shaped
  builder over a retained `Element` tree) for chrome and plugin-authored widgets.
- **`loom-render`** — terminal render pipeline: glyph atlas, text shaping,
  ligatures, font fallback, scene IR.
- **`loom-gpu`** — GPU backend selection: blade (Vulkan/Metal), OpenGL, DX11.
  `render.backend = "auto"` picks blade with a GL fallback.
- **`loom-term`** — terminal emulation + PTY management (forked
  `alacritty_terminal`): escape/OSC handling, Kitty/Sixel graphics, OSC 133
  shell integration.
- **`loom-protocol`** — binary client↔server protocol: message types,
  length-prefixed framing, version negotiation, packed cells, and full-sync vs
  cell-delta updates.
- **`loom-layout`** — column/workspace/tile layout engine
  (`WorkspaceSet → Workspace → Column → Tile(pane_id)`) with proportional/fixed
  column widths and viewport/camera helpers.
- **`loom-anim`** — low-level animation primitives: critically-damped springs,
  easing curves, bezier.
- **`loom-motion`** — style-property transitions for `loom-ui`, built on
  `loom-anim`.
- **`loom-input`** — leader-key state machine and key→action mapping (Kitty
  keyboard protocol + legacy VT encoding).
- **`loom-config`** — TOML config loading, theming, validation, and path
  resolution (XDG on Unix). Consumed by both binaries.
- **`loom-session`** — session persistence and agent-aware restore via atomic
  file writes.
- **`loom-procinfo`** — cross-platform foreground-process detection per PTY;
  drives agent detection and cwd probing.
- **`loom-plugin`** — sandboxed Lua 5.4 plugin engine (`loom.on` event API),
  server-owned and single-threaded.
- **`loom-integration-tests`** — cross-crate integration tests.

## 3. Control & data flow

### Keypress → shell
1. The winit event loop (`crates/loom`) receives a key event.
2. `loom-input` resolves it through the leader state machine into a high-level
   action or raw input.
3. Input-bearing actions become a `loom-protocol` `ClientMessage` pushed onto a
   channel.
4. A client I/O thread frames it and writes to the server socket.
5. The server decodes the frame and hands the bytes to `loom-term`, which writes
   them into the target PTY.

### PTY output → pixels
1. `loom-term` drains PTY output into the emulator grid for each pane.
2. On a periodic tick the server scans grids for damage, producing line deltas
   or full-pane snapshots.
3. Changes are encoded via `loom-protocol` and pushed to connected clients.
4. The client decodes frames and updates the `loom-app` pane grids.
5. `loom-render` turns the logical views into a scene (glyph atlas + batched
   draws).
6. `loom-gpu` submits to the selected backend; winit presents the frame.

Animation (`loom-anim` / `loom-motion`) and layout (`loom-layout`) update
camera/column parameters each frame before rendering.

## 4. Conventions & where to look

- Config options, defaults, and validation ranges:
  `crates/loom-config/src/schema.rs`.
- Protocol message definitions: `crates/loom-protocol/src/message.rs`.
- Client entry: `crates/loom/src/main.rs`; server entry:
  `crates/loom-server/src/main.rs`.
- Key encoding (Kitty + legacy): `crates/loom/src/app/key_encode/mod.rs`.
- Patched upstreams (`winit`, `alacritty_terminal`, `vte`) are pulled from forks
  on the `loom-patches` branch via `[patch.crates-io]` in the root `Cargo.toml`.
- Run `cargo fmt` and `cargo clippy --workspace --all-targets` before sending a
  change.
