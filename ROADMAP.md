# Ciri Roadmap

## Protocol & Performance

- [ ] Encode: write frames directly to writer instead of intermediate `Vec<u8>` (gather write / `write_vectored`)
- [ ] Decode: zero-copy `CellDelta` — borrow `&[PackedCell]` from payload buffer instead of allocating `Vec<PackedCell>`
- [ ] Server: reuse per-client frame buffer (`Vec<u8>`) across ticks, avoid 60fps allocation churn
- [ ] Client: on delta receive, memcpy cells directly into `ClientPaneGrid.cells` — skip intermediate `DamageRegion`
- [x] Flow control: generation/ack closed loop — client sends Ack, server tracks `last_acked_generation`, disconnects slow clients (100+ consecutive failures)
- [x] Security: validate grid dimensions in `decode_full_pane_sync` (max 10M cells), RLE decode overflow protection
- [x] Security: Unix socket permissions set to 0o700 (owner-only)
- [x] Security: viewport/cell dimension validation in handshake (finite, >0, bounded)
- [x] Per-client viewport: smallest-client-wins strategy (tmux model), no global viewport override
- [x] Pane encapsulation: daemon no longer imports alacritty_terminal directly
- [x] Client send reliability: blocking send for critical messages, lossy for Ack/MouseInput
- [ ] Evaluate switching control messages from msgpack to a zero-copy format (flatbuffers / custom binary)
- [ ] Event processing budget per frame (prevent input lag under high server throughput)
- [ ] Server tick lock optimization (snapshot-then-release pattern)

## Layout & Navigation

- [x] 2D layout model: `LayoutState` is now `rows: Vec<RowState>` — protocol, server, and client share the same 2D structure
- [x] FocusUp / FocusDown: navigate between workspace rows
- [x] SplitDown: creates a new row (not just a column)
- [x] SwitchWorkspace: server-authoritative (was client-only)
- [x] ColumnWidth increase/decrease: synced to server via SetColumnWidth
- [x] Per-pane resize: PTY resize from full layout tree (not just visible tiles), respecting individual column widths
- [x] Empty row cleanup: focus moves to previous row when active row's last pane exits
- [x] Config-driven column_gap / row_gap (no more hardcoded 8px)
- [x] view_offset_x removed from protocol (per-client camera state)

## Rendering

- [x] Terminal semantics: BOLD, DIM, INVERSE, UNDERLINE, STRIKEOUT, HIDDEN flags fully handled
- [x] Cursor shapes: Block, HollowBlock, Beam (2px vertical), Underline (2px horizontal)
- [ ] **Italic / bold font variants** — flag is tracked but glyph_cache only rasterizes regular style
- [ ] **Underline variants** (curly, dotted, dashed) — neovim/helix LSP diagnostics use curly underline
- [ ] Text shaping (harfbuzz) for ligatures and combining marks
- [ ] **Unicode grapheme clustering** — currently 1 char per cell (4 bytes), multi-codepoint emoji breaks
- [ ] Damage tracking (dirty rectangle optimization)
- [ ] Color emoji support
- [ ] **Scrollbar** — track + thumb rects, mouse drag

## Client Architecture

- [x] Split main.rs into app/mod.rs (state), app/sync.rs (server events), app/input_handler.rs (actions), app/render.rs (rendering)
- [x] Idle-aware event loop: `ControlFlow::Wait` when no animations/updates, saves power
- [x] Preserve scrollback on disconnect (grids not cleared, validated on reconnect)
- [x] Config hot-reload: font change notifies server of new cell dimensions, leader key updates live

## Multi-Client

- [x] Size strategy: smallest viewport / cell dims wins (tmux model)
- [ ] Per-client cursor visibility (only active client shows cursor in focused pane)

## Terminal Features

- [x] Scrollback buffer support over the protocol (attach includes up to 1000 lines)
- [x] Clipboard: OSC 52 write (server → client forwarding)
- [x] Clipboard: left-click select, right-click copy (Ghostty-style)
- [x] Clipboard: Ctrl+Shift+C / Ctrl+Shift+V, auto-copy on selection release
- [ ] **Scrollback search** (Ctrl+F) — highlight + navigation
- [ ] **Double-click word selection** — configurable word boundary chars
- [ ] **Bell notification** forwarding (Event::Bell → system notification)
- [ ] **IME preedit rendering** — draw candidate overlay at cursor position
- [ ] **URL / link detection** — clickable links with underline on hover
- [ ] **Shell integration** — prompt marking, semantic zones
- [ ] Sixel / Kitty image protocol support
- [ ] **Kitty keyboard protocol** — currently standard xterm keycodes only

## Session Persistence

- [x] 2D session state: saves all workspace rows (not just active row)
- [x] Atomic writes (temp file + rename)
- [x] Session restore on daemon startup
- [x] Delete stale session on clean exit (don't save empty layout)
- [x] Session name validation on all paths (save, restore, delete, connect, daemon)
- [x] Scrollback included in attach (up to 1000 lines)
- [ ] Auto-save on layout changes
- [ ] Schema versioning for forward compatibility

## Platform

- [ ] Windows: named pipes (`\\.\pipe\`) as alternative to TCP localhost
- [ ] macOS: test and fix Unix domain socket path handling
- [ ] Background opacity / blur
