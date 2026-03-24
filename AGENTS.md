# Ciri – Agent Guide

This repository is a Rust workspace containing a GPU-accelerated terminal emulator with a client–server architecture. The goal of this guide is to help future agents quickly understand how to build, run, and navigate the codebase.

## 1. Core Development Commands

All commands below assume the workspace root: `/home/linxy/repo/ciri`.

### Build

- Build the entire workspace (debug):  
  `cargo build`
- Build optimized binaries:  
  `cargo build --release`

### Run

The workspace defines two primary binaries:

- `ciri` – GUI client (winit + wgpu)
- `ciri-server` – background daemon

Typical workflows:

- Run the server daemon:  
  `cargo run -p ciri-server -- [server-args]`
- Run the client:  
  `cargo run -p ciri -- [client-args]`

On Unix, the server listens on a Unix domain socket; on Windows it uses a loopback TCP socket. The client and server communicate using the shared `ciri-protocol` crate.

### Test

- Run all tests in the workspace:  
  `cargo test`
- Run tests for a single crate (example with `ciri-input`):  
  `cargo test -p ciri-input`
- Run a single test (by name or module path) within a crate:  
  `cargo test -p ciri-input <test_name_or_mod_path>`

### Lint and Format

If `clippy` is installed:

- Lint all targets in the workspace:  
  `cargo clippy --workspace --all-targets`

Rustfmt:

- Format the entire workspace:  
  `cargo fmt`

## 2. Workspace Layout and Crate Responsibilities

### Top-Level Structure

- `Cargo.toml` – workspace root; defines shared dependencies and profiles.
- `crates/` – main Rust crates (binaries and libraries).
- `config/` – configuration defaults and related assets.
- `assets/` – fonts, images, and other runtime assets.
- `patches/winit/` – patched `winit` crate used via `[patch.crates-io]` to fix Windows 11 multi-monitor DPI drag issues.
- `ARCHITECTURE_REVIEW.md` – detailed architecture and quality review.
- `FEATURES.md`, `PRD.md`, `ROADMAP.md`, `docs/PRODUCT_VISION.md` – product-level design and planning documents.

### Binary Crates

- `crates/ciri` (`bin: ciri`)  
  GUI client application. Owns the winit event loop, integrates input (`ciri-input`), layout (`ciri-layout`), animation (`ciri-anim`), rendering (`ciri-render` + `ciri-gpu`), configuration (`ciri-config`), and client-side session handling (`ciri-session`). Communicates with the server using `ciri-protocol` over a local socket.

- `crates/ciri-server` (`bin: ciri-server`)  
  Background daemon running on a tokio runtime. Manages PTYs via `ciri-term`, maintains server-side layout (`ciri-layout`) and sessions (`ciri-session`), applies configuration (`ciri-config`), and encodes/decodes messages with `ciri-protocol`. Responsible for accepting client connections and pushing terminal updates.

### Library Crates (Subsystems)

- `crates/ciri-render`  
  GPU rendering pipeline for terminal content. Uses `wgpu` to render glyphs and rectangles with a glyph atlas, shelf-allocated texture, and sRGB-aware shaders. Consumes logical terminal views from the client and produces render passes.

- `crates/ciri-gpu`  
  Thin abstraction layer over `wgpu` and related GPU resources, shared by rendering code to keep low-level device/swapchain setup separate from higher-level rendering logic.

- `crates/ciri-term`  
  Terminal emulation and PTY management on the server. Wraps `alacritty_terminal` for escape sequence handling and `portable-pty` for cross-platform PTY spawning, then exposes a higher-level API used by `ciri-server` to drive terminal grids and extract damage.

- `crates/ciri-layout`  
  Layout engine for workspaces, columns, and panes. Implements a row-major tiling model where `WorkspaceSet → Workspace → Column → Tile(pane_id)`, with proportional and fixed column widths, viewport/camera handling, and helpers for determining which panes are visible.

- `crates/ciri-anim`  
  Animation engine, centered around analytically solved critically damped springs. Used by layout and UI to animate camera movement, column widths, and other properties in a frame-independent way.

- `crates/ciri-input`  
  Input and keybinding layer. Provides a leader-key state machine, modifier-aware key combos, and mapping from winit key events to high-level actions (e.g., focus movement, pane management, scroll commands) understood by the client and server.

- `crates/ciri-config`  
  Configuration loading and theming. Handles TOML-based config files, theme presets and overrides, and config path resolution (XDG on Unix, platform-appropriate paths elsewhere). Both client and server consume this crate.

- `crates/ciri-protocol`  
  Shared binary protocol definitions between client and server. Defines message types, framing (`[tag][len_le][payload]`), version negotiation, and efficient cell representations (`PackedCell`) used for full-grid syncs and damage-based updates.

- `crates/ciri-session`  
  Session persistence utilities used primarily by the server and client startup/shutdown flows. Handles saving and restoring workspaces, panes, and related metadata via atomic file writes.

- `crates/ciri-term`, `crates/ciri-session`, `crates/ciri-config`, `crates/ciri-layout`, `crates/ciri-protocol`  
  Shared logic used across the two binaries as described above; there are no circular dependencies, and the overall crate graph forms a clean DAG.

## 3. High-Level Control and Data Flow

### From Keypress to Shell

1. **winit event loop** (in `crates/ciri`) receives keyboard input events.
2. Events are converted into `ciri-input` key combos and mapped to high-level actions (e.g., send input to pane, focus change).
3. For actions that send bytes to the shell, the client builds a `ClientMessage::Input` (from `ciri-protocol`) and pushes it into a crossbeam channel.
4. A dedicated client I/O thread reads from this channel, encodes frames with `ciri-protocol`, and writes them to the server over the local socket.
5. On the server side (`ciri-server`), the tokio reader task decodes frames and dispatches them to `ciri-term`, which writes the bytes into the appropriate PTY.

### From PTY Output to Pixels

1. `ciri-term` continuously drains PTY output into `alacritty_terminal`, which mutates an in-memory grid for each pane.
2. On each tick (e.g., every 16ms), `ciri-server` scans the grids for changed regions, producing line-based damage and, when needed, full-pane snapshots.
3. These changes are encoded via `ciri-protocol` as either full syncs or cell deltas and sent to connected clients.
4. The client’s server-I/O thread decodes incoming frames and forwards them to the main thread via a crossbeam channel.
5. `ciri` updates its `ClientPaneGrid` structures and derives logical `TerminalView` representations.
6. `ciri-render` converts these views into draw commands, ensuring all required glyphs are present in the atlas and batching glyph/rect draws.
7. `ciri-gpu` submits the corresponding `wgpu::RenderPass` to the GPU, and `winit` presents the final frame.

Animations (`ciri-anim`) and layout changes (`ciri-layout`) hook into this loop by updating camera/column parameters each frame before rendering.

## 4. Key Reference Documents for Agents

When making non-trivial changes, consult these documents first:

- `ARCHITECTURE_REVIEW.md` – in-depth review of each crate, including strengths, weaknesses, and security/robustness concerns.
- `FEATURES.md` – current and planned feature set; useful for aligning new work with roadmap items.
- `PRD.md` – product requirements and constraints.
- `ROADMAP.md` – prioritized roadmap and sequencing of major initiatives.
- `docs/PRODUCT_VISION.md` – long-term product vision and positioning.

These documents contain additional details (e.g., security concerns in `ciri-protocol` and `ciri-server`, performance considerations in `ciri-render`, and configuration limitations in `ciri-config`) that are important when planning larger refactors or feature work.
