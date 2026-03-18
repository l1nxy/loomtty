# Ciri Roadmap

## Protocol & Performance

- [ ] Encode: write frames directly to writer instead of intermediate `Vec<u8>` (gather write / `write_vectored`)
- [ ] Decode: zero-copy `CellDelta` — borrow `&[PackedCell]` from payload buffer instead of allocating `Vec<PackedCell>`
- [ ] Server: reuse per-client frame buffer (`Vec<u8>`) across ticks, avoid 60fps allocation churn
- [ ] Client: on delta receive, memcpy cells directly into `ClientPaneGrid.cells` — skip intermediate `DamageRegion`
- [ ] Flow control: use `last_acked_generation` to skip or throttle updates for slow clients
- [ ] Evaluate switching control messages from msgpack to a zero-copy format (flatbuffers / custom binary)

## Layout & Navigation

- [ ] Vertical tiling: support multiple panes within a single column
- [ ] FocusUp / FocusDown: implement proper vertical navigation once vertical tiling lands
- [ ] Per-pane resize: PTY resize per-pane instead of uniform grid, respecting individual column widths

## Multi-Client

- [ ] Size strategy for multiple clients (smallest viewport / active client / independent viewports)
- [ ] Per-client cursor visibility (only active client shows cursor in focused pane)

## Terminal Features

- [ ] Scrollback buffer support over the protocol
- [ ] Clipboard integration (OSC 52)
- [ ] Sixel / image protocol support
- [ ] Bell notification forwarding

## Platform

- [ ] Windows: evaluate named pipes (`\\.\pipe\`) as alternative to TCP localhost for lower overhead
- [ ] macOS: test and fix Unix domain socket path handling
- [ ] Session persistence: save/restore sessions across server restarts
