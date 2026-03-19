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

- [ ] Encode: write frames directly to writer instead of intermediate `Vec<u8>` (gather write / `write_vectored`)
- [ ] Decode: zero-copy `CellDelta` — borrow `&[PackedCell]` from payload buffer instead of allocating `Vec<PackedCell>`
- [ ] Server: reuse per-client frame buffer (`Vec<u8>`) across ticks, avoid 60fps allocation churn
- [ ] Client: on delta receive, memcpy cells directly into `ClientPaneGrid.cells` — skip intermediate `DamageRegion`
- [x] Flow control: generation/ack closed loop
- [x] Security: validate grid dimensions, RLE decode overflow protection
- [x] Security: Unix socket permissions 0o700
- [x] Security: viewport/cell dimension validation in handshake
- [x] Per-client viewport: smallest-client-wins strategy
- [x] Pane encapsulation: daemon no longer imports alacritty_terminal directly
- [x] Client send reliability: blocking send for critical messages, lossy for Ack/MouseInput
- [ ] Evaluate switching control messages to zero-copy format (flatbuffers / custom binary)
- [ ] Event processing budget per frame (prevent input lag under high throughput)
- [ ] Server tick lock optimization (snapshot-then-release pattern)

## Rendering

- [x] Terminal semantics: BOLD, DIM, INVERSE, UNDERLINE, STRIKEOUT, HIDDEN flags
- [x] Cursor shapes: Block, HollowBlock, Beam, Underline
- [ ] **Italic / bold font variants** — flag tracked but not rendered
- [ ] **Underline variants** (curly, dotted, dashed) — neovim/helix LSP diagnostics use curly underline
- [ ] Text shaping (harfbuzz) for ligatures and combining marks
- [ ] **Unicode grapheme clustering** — currently 1 char per cell, multi-codepoint emoji breaks
- [ ] Damage tracking (dirty rectangle optimization)
- [ ] Color emoji support
- [ ] **Scrollbar** — track + thumb rects, mouse drag
- [ ] **Pane open/close animation** — fade-in/slide on create, snapshot + fade-out on close
- [ ] **Focus ring** — configurable width, color, corner radius, inactive pane dimming
- [ ] **Inactive pane opacity** — GPU-level dimming of unfocused panes

## Client Architecture

- [x] Split main.rs into app/mod.rs, sync.rs, input_handler.rs, render.rs
- [x] Idle-aware event loop: `ControlFlow::Wait` when no animations
- [x] Preserve scrollback on disconnect
- [x] Config hot-reload: font change, leader key, keybindings
- [ ] **Touchpad gestures** — three-finger swipe to scroll columns, up/down for workspace switch

## Multi-Client

- [x] Size strategy: smallest viewport / cell dims wins
- [ ] Per-client cursor visibility (only active client shows cursor in focused pane)

## Terminal Features

- [x] Scrollback buffer support (attach includes up to 1000 lines)
- [x] Clipboard: OSC 52, left-click select, right-click copy, Ctrl+Shift+C/V
- [x] Double-click word selection
- [x] URL / link detection — clickable links with underline on hover
- [x] **Scrollback search** (Ctrl+Shift+F) — highlight + navigation
- [ ] **Broadcast input** (Leader+b) — send keystrokes to all visible panes simultaneously
- [ ] **Shell integration** — OSC 133 prompt marking, semantic zones, command output folding
- [ ] **Bell notification** forwarding
- [ ] **IME preedit rendering** — draw candidate overlay at cursor position
- [ ] Sixel / Kitty image protocol support
- [ ] **Kitty keyboard protocol**

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

- [ ] Windows: named pipes as alternative to TCP localhost
- [ ] macOS: test and fix Unix domain socket path handling
- [ ] Background opacity / blur
