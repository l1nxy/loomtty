# Ciri Roadmap

## Layout Evolution

### Phase 1: Layout 核心

- [x] 2D layout model: workspaces × columns, protocol/server/client shared structure
- [x] FocusUp / FocusDown: navigate between workspaces
- [x] NewWorkspaceBelow: creates a new workspace (not just a column)
- [x] SwitchWorkspace: server-authoritative
- [x] ColumnWidth increase/decrease: synced to server via SetColumnWidth
- [x] Per-pane resize: PTY resize from full layout tree, respecting individual column widths
- [x] Empty workspace cleanup: focus moves to previous workspace when last pane exits
- [x] Config-driven column_gap / workspace_gap
- [x] view_offset_x removed from protocol (per-client camera state)
- [x] Rename "row" → "workspace" across entire codebase (clarity)
- [x] **Configurable width presets** — user-defined preset list + cycle key (`r`/`Shift+R`), supports proportion and fixed-pixel
- [x] **Center focused column strategy** — `always` / `on-overflow` / `never` (`[layout] center_focused_column`)
- [x] **Smart adjacent resize** — drag border resizes both neighbors, total width conserved

### Phase 2: Column 内垂直堆叠

- [x] **Column → Vec\<Tile\>** — one column holds multiple panes stacked vertically
- [x] **TileHeight enum** — `Auto { weight }` (flex-like) / `Fixed(px)`
- [x] **Consume** (Leader+c) — absorb right column's pane into current column as new tile
- [x] **Expel** (Leader+v) — eject active tile from column into a new column to the right
- [x] **Column-internal j/k** — navigate tiles within a column; fall through to workspace switch when single-tile
- [x] **Tile height resize** — drag horizontal border between tiles within a column (RowResize cursor)
- [x] **Protocol update** — tile weight synced in LayoutState/SessionState (real weight, not hardcoded 1.0)
- [x] **Server PTY resize** — per-tile height based on weight distribution within column

## Protocol & Performance

- [x] Encode: gather write — batch-drain frames in writer task, single flush per batch
- [x] Decode: zero-copy `CellDelta` — `CellDeltaBorrowed` borrows `&[PackedCell]` from payload buffer
- [x] Server: per-client frame buffer — Bytes refcount sharing for broadcasts, frame pool for encoding
- [x] Client: flat viewport buffer — direct memcpy into contiguous Vec<PackedCell>, skip VecDeque/Vec indirection
- [x] Flow control: generation/ack closed loop
- [x] Security: validate grid dimensions, RLE decode overflow protection
- [x] Security: Unix socket permissions 0o700
- [x] Security: viewport/cell dimension validation in handshake
- [x] Per-client viewport: smallest-client-wins strategy
- [x] Pane encapsulation: daemon no longer imports alacritty_terminal directly
- [x] Client send reliability: blocking send for critical messages, lossy for Ack/MouseInput
- [x] Evaluate switching control messages to zero-copy format — decided: keep msgpack (cold path), custom binary+bytemuck (hot path); flatbuffers rejected (codegen overhead, minimal wire savings since cell data dominates)
- [x] Event processing budget per frame — client: 200 events/frame budget in sync.rs; server: PTY processing budget per tick
- [x] Server tick lock optimization — two-phase snapshot-then-release: PTY/damage under lock, encoding unlocked, revalidate on send

## Rendering

- [x] Terminal semantics: BOLD, DIM, INVERSE, UNDERLINE, STRIKEOUT, HIDDEN flags
- [x] Cursor shapes: Block, HollowBlock, Beam, Underline
- [x] **Italic / bold font variants** — FontStyle enum with proper font chain lookup and synthesis
- [x] **Underline variants** (curly, dotted, dashed) — double, curly (sine wave), dotted, dashed all rendered
- [x] **Text shaping** (rustybuzz) — ligature detection, grapheme shaping with glyph ID output
- [x] **Unicode grapheme clustering** — unicode-segmentation crate, ZWJ/variation selector/combining mark support
- [x] Damage tracking (dirty flag + per-row dirty flags, not full dirty-rect yet)
- [x] Color emoji support — RGBA atlas with swash Content::Color detection
- [x] **Scrollbar** — visual track + thumb (no mouse drag yet)
- [x] **Pane open/close animation** — fade + slide variants (SlideUp/Down/Left/FadeSlideUp), configurable duration
- [x] **Focus ring** — Glow / Dashed / Solid styles, configurable width and color, active/inactive border colors
- [x] **Inactive pane opacity** — spring-animated focus transitions, GPU-level dimming

## Client Architecture

- [x] Split main.rs into app/mod.rs, sync.rs, input_handler.rs, render.rs
- [x] Idle-aware event loop: `ControlFlow::Wait` when no animations
- [x] Preserve scrollback on disconnect
- [x] Config hot-reload: font change, leader key, keybindings
- [x] **Touchpad gestures** — smooth scrolling, shift+scroll workspace switch, pinch for overview zoom

## Multi-Client

- [x] Size strategy: smallest viewport / cell dims wins
- [ ] Per-client cursor visibility (only active client shows cursor in focused pane)

## Terminal Features

- [x] Scrollback buffer support (attach includes up to 1000 lines)
- [x] Clipboard: OSC 52, left-click select, right-click copy, Ctrl+Shift+C/V
- [x] Double-click word selection
- [x] URL / link detection — clickable links with underline on hover
- [x] **Scrollback search** (Ctrl+Shift+F) — highlight + navigation
- [x] **Broadcast input** (Leader+b) — send keystrokes to all visible panes simultaneously
- [x] **Shell integration** — OSC 133 prompt marking, semantic zones with SemanticZone/ShellState tracking
- [x] **Bell notification** — visual bell with 150ms fade-out animation
- [x] **IME preedit rendering** — candidate overlay with underline and cursor at proper position
- [x] Kitty image protocol — multi-chunk accumulation, placeholder rendering with borders
- [ ] Sixel image protocol
- [x] **Kitty keyboard protocol** — CSI u format with per-pane detection

## Session Persistence

- [x] 2D session state: saves all workspaces
- [x] Atomic writes (temp file + rename)
- [x] Session restore on daemon startup
- [x] Delete stale session on clean exit
- [x] Session name validation
- [x] Scrollback included in attach (up to 1000 lines)
- [x] Auto-save on layout changes (debounced 250ms)
- [ ] Schema versioning for forward compatibility
- [ ] **Layout templates** — TOML files defining startup layouts, session ↔ template round-trip

## Ecosystem

- [ ] **Command palette** (fuzzy finder) — search panes by title/CWD/command, switch sessions, execute actions
- [ ] **Remote session** — local client connects to remote server via SSH tunnel, mixed local+remote panes
- [ ] **Niri native integration** — leverage niri IPC when running under niri compositor
- [ ] IPC / CLI interface for external scripting and automation

## Platform

- [x] Windows: named pipes (`\\.\pipe\ciri-server`) replacing TCP localhost
- [ ] macOS: test and fix Unix domain socket path handling
- [ ] Background opacity / blur
