# Ciri Roadmap

## Protocol & Performance

- [ ] Encode: write frames directly to writer instead of intermediate `Vec<u8>` (gather write / `write_vectored`)
- [ ] Decode: zero-copy `CellDelta` — borrow `&[PackedCell]` from payload buffer instead of allocating `Vec<PackedCell>`
- [ ] Server: reuse per-client frame buffer (`Vec<u8>`) across ticks, avoid 60fps allocation churn
- [ ] Client: on delta receive, memcpy cells directly into `ClientPaneGrid.cells` — skip intermediate `DamageRegion`
- [x] Flow control: generation/ack closed loop — client sends Ack, server tracks `last_acked_generation`, disconnects slow clients (100+ consecutive failures)
- [x] Security: validate grid dimensions in `decode_full_pane_sync` (max 10M cells), RLE decode overflow protection
- [x] Security: Unix socket permissions set to 0o700 (owner-only)
- [ ] Evaluate switching control messages from msgpack to a zero-copy format (flatbuffers / custom binary)

## Layout & Navigation

- [x] 2D layout model: `LayoutState` is now `rows: Vec<RowState>` — protocol, server, and client share the same 2D structure
- [x] FocusUp / FocusDown: navigate between workspace rows
- [x] SplitDown: creates a new row (not just a column)
- [x] Per-pane resize: PTY resize from full layout tree (not just visible tiles), respecting individual column widths
- [x] Empty row cleanup: focus moves to previous row when active row's last pane exits

## Rendering

- [x] Terminal semantics: BOLD, DIM, INVERSE, UNDERLINE, STRIKEOUT, HIDDEN flags fully handled
- [x] Cursor shapes: Block, HollowBlock, Beam (2px vertical), Underline (2px horizontal)
- [ ] Text shaping (harfbuzz) for ligatures and combining marks
- [ ] Damage tracking (dirty rectangle optimization)
- [ ] Color emoji support

## Client Architecture

- [x] Split main.rs into app/mod.rs (state), app/sync.rs (server events), app/input_handler.rs (actions), app/render.rs (rendering)
- [x] Idle-aware event loop: `ControlFlow::Wait` when no animations/updates, saves power
- [x] Preserve scrollback on disconnect (grids not cleared, validated on reconnect)
- [x] Config hot-reload: font change notifies server of new cell dimensions, leader key updates live
- [ ] Event processing budget per frame (prevent input lag under high server throughput)
- [ ] Server tick lock optimization (snapshot-then-release pattern)

## Multi-Client

- [ ] Size strategy for multiple clients (smallest viewport / active client / independent viewports)
- [ ] Per-client cursor visibility (only active client shows cursor in focused pane)

## Terminal Features

- [x] Scrollback buffer support over the protocol
- [ ] Clipboard integration (OSC 52)
- [ ] Sixel / image protocol support
- [ ] Bell notification forwarding
- [ ] IME preedit rendering

## Session Persistence

- [x] 2D session state: saves all workspace rows (not just active row)
- [x] Atomic writes (temp file + rename)
- [ ] Auto-save on layout changes
- [ ] Schema versioning for forward compatibility

## Platform

- [ ] Windows: evaluate named pipes (`\\.\pipe\`) as alternative to TCP localhost for lower overhead
- [ ] macOS: test and fix Unix domain socket path handling
