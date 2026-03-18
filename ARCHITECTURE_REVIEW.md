# Ciri Architecture Review

> Date: 2026-03-18 | Branch: `dev/client-server-architecture`
>
> Reviewed against: Alacritty, WezTerm, Kitty, tmux, mosh, RDP

---

## Overall Verdict

The architecture is **well-thought-out and follows industry practices** — not a naively hand-rolled design. Core decisions (VT parsing via alacritty\_terminal, GPU rendering via wgpu, analytical spring animation, client-server damage-based sync) are all correct choices backed by established approaches.

| Module | Grade | Summary |
|--------|-------|---------|
| ciri-render | A- | Correct GPU pipeline, clean module boundaries, good font fallback |
| ciri-protocol | B | Efficient framing & damage sync, but critical security & validation gaps |
| ciri-term | B+ | Proper delegation to alacritty\_terminal, good PTY abstraction |
| ciri-server | B- | Sound tick-based design, but security & concurrency concerns |
| ciri (client) | B+ | Good buffer reuse & scrollback model, needs error handling |
| ciri-layout | B+ | Clean hierarchy, well-tested, row-major constraint is intentional |
| ciri-config | B | Works but lacks validation; silent failures on bad input |
| ciri-input | B+ | Correct leader-key state machine, good modifier support |
| ciri-anim | A- | Excellent spring physics (analytical solution), minor config wiring gaps |
| ciri-session | B | Atomic writes, but incomplete state capture |
| **Overall Architecture** | **A-** | **Clean crate graph, correct client-server split, good tech choices** |

---

## 1. ciri-render (Rendering)

### What it does right

- **GPU-accelerated via wgpu** with two separate pipelines (glyph instancing + colored rectangles) — same approach as Alacritty/WezTerm.
- **Glyph atlas** with shelf-based bin packing, lazy rasterization via `swash`, proper UV coordinate clipping.
- **Font fallback chain**: uses `fontconfig` + `FcFontSort` on Unix, `fontdb` scan on Windows. Better than many terminal emulators.
- **sRGB color space** handling is correct in both glyph and rect shaders.
- **Module separation** is clean: `terminal.rs` does pure data transformation (no wgpu import), `glyph_cache.rs` and `rect.rs` are independent pipelines, `renderer.rs` ties them together.
- **Wide character handling**: proper `FLAG_WIDE_CHAR` / `FLAG_WIDE_CHAR_SPACER` tracking.
- **Baseline positioning** uses ascent-relative calculation, not magic multipliers.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| No text shaping | Minor | `glyph_cache.rs` | Character-by-character rasterization. No harfbuzz — ligatures and combining marks won't render correctly. Acceptable for monospace MVP, but Alacritty/WezTerm/Kitty all use harfbuzz. |
| No damage tracking | Minor | `terminal.rs` | Full grid re-scanned every frame even if only 1 cell changed. At 80x24 (1920 cells) this is negligible, but becomes relevant at 4K resolutions (200x50+). WezTerm/Kitty track dirty regions. |
| No atlas eviction | Minor | `glyph_cache.rs:481` | Once atlas is full, new glyphs return `GlyphEntry::EMPTY`. No LRU eviction. Unlikely to hit in typical sessions, but possible with many scripts/emoji. |
| No color emoji | Suggestion | `glyph_cache.rs` | Single R8Unorm atlas (monochrome alpha only). No CBDT/COLR table support. |
| NDC conversion inconsistency | Suggestion | `glyph_cache.rs:601` vs `rect.rs:114` | Glyph pipeline pre-converts to NDC on CPU (main.rs), rect pipeline converts during render. Both work but approach is inconsistent. Consider a shared utility or uniform buffer. |

### Comparison to industry

| Feature | ciri | Alacritty | WezTerm | Kitty |
|---------|------|-----------|---------|-------|
| GPU acceleration | wgpu | GL/Metal | GPU | GPU |
| Text shaping | No | harfbuzz | harfbuzz | Custom |
| Glyph cache | Custom atlas | Custom | Glyphon + cache | GPU atlas |
| Font fallback | Good (fontconfig) | Good | Good | Excellent |
| Damage tracking | No (full redraw) | Partial | Full | Full |
| Ligatures | No | Yes | Yes | Yes |
| Color emoji | No | No | Limited | Yes |
| Sixel/images | No | No | Yes | Yes |

### Recommendation

For MVP: **ready to ship**. For production: add harfbuzz text shaping and damage tracking as next priorities.

---

## 2. ciri-protocol (Wire Protocol)

### What it does right

- **Length-delimited framing** `[tag(1)][len_le(4)][payload(len)]` — clean, no ambiguity. Matches protobuf delimited format.
- **RLE compression** for FullPaneSync — smart for blank-heavy screens.
- **Incremental damage updates** via CellDelta — server tracks per-line damage regions and sends only changed cells.
- **Zero-copy PackedCell** (14 bytes, `#[repr(C, packed)]`, `bytemuck::Pod`) — avoids deserialization overhead.
- **Version negotiation** with `pack_version()` / `check_version()` / `VersionCompat` enum.
- **Safe integer readers** (`read_u16_le`, `read_u32_le`) use bounds-checked `.get()`.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| **No authentication** | CRITICAL | `codec.rs:78-132` | Handshake only exchanges version numbers. Any process can connect to any session. tmux uses socket permissions, mosh uses AES-128 shared secret, RDP uses TLS. |
| **No encryption** | CRITICAL | `codec.rs` | All frames sent plaintext. Acceptable for localhost-only; unacceptable for any remote use. |
| **Grid dimension DoS** | CRITICAL | `codec.rs:441-442` | `decode_full_pane_sync` reads cols/rows from wire without range check. `cols=65535, rows=65535` → 4 billion cells → OOM. Must validate `cols * rows <= MAX_GRID_SIZE`. |
| RLE integer overflow | IMPORTANT | `codec.rs:367-394` | `rle_decode_cells` doesn't check `cells.len() + count <= expected` before appending. Could panic on malformed data. |
| No flow control | IMPORTANT | `daemon.rs:483-491` | `try_send()` silently drops frames when channel is full. No backpressure, no retry, no slow-client detection. |
| No feature negotiation | IMPORTANT | `codec.rs` | Handshake doesn't negotiate capabilities. If server adds new frame type in 0.2.0, old clients will error on unknown tag. |
| Silently dropped frames | IMPORTANT | `daemon.rs:488` | Frame dropped if channel full — client gets out of sync. Should either slow down or disconnect slow clients. |
| bytemuck alignment | IMPORTANT | `codec.rs:171-179` | `cast_slice` on `#[repr(C, packed)]` assumes alignment. Potential UB on strict-alignment architectures (ARM, SPARC). |
| No reconnection protocol | IMPORTANT | `codec.rs` | On reconnect, client gets full StateSync instead of delta from last generation. Inefficient for large panes. |
| Ack message unused | MINOR | `message.rs:159` | `ClientMessage::Ack` exists but `last_acked_generation` is written and never read. |
| SetColumnWidth not validated | MINOR | `message.rs:153` | `proportion: f64` not validated for `0 <= proportion <= 1`. |

### Recommendation

**Immediate**: validate grid dimensions in `decode_full_pane_sync`, add bounds check to RLE decode. **Short-term**: implement backpressure/flow control, add feature negotiation. **Before remote use**: add authentication + TLS.

---

## 3. ciri-term (Terminal Emulation & PTY)

### What it does right

- **Delegates to `alacritty_terminal`** for VT100/xterm escape sequence parsing — uses the `vte` state machine internally. **Not hand-rolling** parsing (which is extremely error-prone).
- **Cross-platform PTY** via `portable-pty` (Unix pty, Windows ConPty).
- **Background reader thread** with bounded `sync_channel(8)` prevents blocking the main thread.
- **Client-side scrollback** — server sends only viewport, client manages history in `VecDeque`. Smart architectural decision.
- **PackedCell** zero-copy serialization (14 bytes POD).
- **PtyEventListener** correctly implements `alacritty_terminal::event::EventListener` for DA responses and title updates.
- **EOF tracking** via `AtomicBool` — clean shutdown detection.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| Unnecessary `Arc<Mutex<Term>>` | MEDIUM | `pane.rs:31` | Pane is stored in `HashMap<u64, Pane>` and never shared across threads. `Arc` adds atomic overhead on every lock/unlock in the hot path (process_pty_output, called every 16ms). Should be just `Term` or `Mutex<Term>`. |
| Reader thread lifecycle | MEDIUM | `pty.rs:160-173` | Drop impl calls `child.kill()` but never joins the reader thread. Relies on implicit fd closure. Could accumulate zombie threads if spawning panes rapidly. Should store `JoinHandle` and join with timeout. |
| Heap allocation per PTY read | MEDIUM | `pty.rs:101` | `buf[..n].to_vec()` allocates a new `Vec<u8>` on every read (65KB buffers, potentially 1000+ allocs/sec for high-bandwidth output). Consider `bytes::BytesMut` or a buffer pool. |
| Unsafe shell detection buffer | MEDIUM | `pty.rs:20-41` | `getpwuid_r` uses hardcoded 4096-byte buffer instead of `sysconf(_SC_GETPW_R_SIZE_MAX)`. May fail on systems with large passwd entries. |
| Full-line damage expansion | LOW-MEDIUM | `pane.rs:216-230` | Damage always expanded to full line width. Throws away granular column info. Bandwidth-inefficient for wide terminals. Conservative but correct. |
| No resize bounds checking | LOW | `pane.rs:162-171` | No validation that cols/rows > 0. PTY resize failure is silent. |
| Dirty flag undocumented | LOW | `pane.rs:54` | `dirty` flag semantics are implicit — cleared externally by daemon after reading damage. Should document the contract. |

### Recommendation

**High priority**: remove `Arc` wrapper (Pane is never shared). **Medium**: add explicit reader thread join in Drop, consider buffer reuse for PTY reads.

---

## 4. ciri-server (Daemon)

### What it does right

- **Tick-based processing** (16ms) — processes all PTY output, accumulates damage, sends frames. Clean game-loop style.
- **Per-client damage accumulators** — each client gets independent damage tracking, allowing late-joining clients to receive full sync.
- **Reader/writer task separation** per client — reader handles incoming messages, writer drains outgoing frame channel.
- **Graceful shutdown** on SIGTERM/SIGINT with session save.
- **Session state management** with workspace layout tracking.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| **Unix socket world-readable** | HIGH | `daemon.rs:422` | `UnixListener::bind()` creates socket with default permissions (typically 0o755). Any user on the system can connect and hijack the session. Must set to 0o700. tmux does this by default. |
| **Windows TCP world-accessible** | HIGH | `daemon.rs:424-426` | Uses `127.0.0.1:PORT` with deterministic port from session name hash. Any local process can connect. Should use named pipes (`\\.\pipe\ciri-*`) instead. |
| Lock hold time exceeds budget | MEDIUM | `daemon.rs:457-668` | Tick loop holds `Arc<Mutex<ServerState>>` while processing ALL panes AND building ALL frames. For 100 panes × 10 clients, lock time could reach 200-300ms (16ms budget). Reader tasks are starved during this time, causing input lag. Should snapshot damage data, release lock, then build frames. |
| Panic-while-locked deadlock | MEDIUM | `daemon.rs:847-895` | If `handle_message()` panics while holding state lock, `cleanup_client()` tries to acquire the same lock → deadlock. Should scope the lock more narrowly. |
| No client spawn limit | MEDIUM | `daemon.rs:725` | `tokio::spawn()` on every accepted connection with no limit. Connection flood → resource exhaustion. Add a `Semaphore` to cap concurrent clients. |
| Writer task force-abort | MEDIUM | `daemon.rs:900` | `write_handle.abort()` without draining pending frames. Client may receive incomplete state. Should close tx channel first, let writer drain, then abort with timeout. |
| try_send failures ignored | MEDIUM | `daemon.rs:488` | Frames silently dropped when client channel is full. No consecutive failure counting, no client eviction. A misbehaving client can fall arbitrarily behind. |
| No PID file | MEDIUM | `daemon.rs:390` | No PID file management. Stale sockets persist if server crashes. `is_session_running()` uses connection probe (slow). |
| No reader timeout/size limit | MEDIUM | `daemon.rs:847-895` | `read_frame()` has no message size limit or read timeout. Malicious client could send gigabyte frame or hold connection indefinitely. |
| Missing double-fork | LOW-MEDIUM | `connection.rs:174-191` | Only `setsid()`, no second fork. Sufficient for most cases, but daemon could theoretically reacquire controlling terminal. |
| Broadcast frame clones | MEDIUM | `daemon.rs:865` | `frame.clone()` for each client on broadcast. Use `Arc<Vec<u8>>` instead. |
| history_sent leak | LOW | `daemon.rs:150-157` | `close_pane()` removes damage map but not `history_sent` entries per client. Minor memory leak. |
| last_acked_generation dead code | LOW | `daemon.rs:71` | Written on Ack receipt but never read. Suggests planned flow control not yet implemented. |

### Recommendation

**Critical**: fix socket permissions to 0o700. **High priority**: reduce lock hold time (snapshot-then-release pattern), fix panic-while-locked, add client spawn limit. **Medium**: implement slow client detection and eviction.

---

## 5. ciri (Client Main)

### What it does right

- **ClientPaneGrid** uses `VecDeque` ring buffer for scrollback — memory-efficient, O(1) push/pop.
- **Buffer reuse** with `std::mem::take()` + `clear()` pattern for glyph and rect buffers — avoids per-frame allocation.
- **Text selection** uses absolute `buffer_row` indices that survive scrollback — correct approach.
- **Exponential backoff reconnection** with configurable max attempts (500ms initial, 10s cap).
- **IME position tracking** from active pane cursor position.
- **UV clipping math** for partially visible glyphs is correct.
- **Cached terminal views** — only rebuild when grid changes.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| `.expect()` on GPU/window init | HIGH | `main.rs:1157,1160` | `create_window().expect()` and `Renderer::new().expect()` — panics with cryptic error on failure. Should show user-friendly error or attempt fallback. |
| State lost on disconnect | MEDIUM-HIGH | `main.rs:247-252` | `pane_grids.clear()` on disconnect throws away all scrollback. Should preserve local state and validate against server on reconnect. |
| Always-on redraw loop | MEDIUM | `main.rs:1046-1051` | `ControlFlow::WaitUntil` set unconditionally every event cycle. Should switch to `ControlFlow::Wait` when idle (no animations, no pending updates) to save power. |
| Unbounded event processing | MEDIUM | `main.rs:177-180` | `process_server_events()` drains entire crossbeam channel before rendering. During high-throughput output, input events wait until all server events are processed. Should limit to time budget (e.g., 5ms) or count limit per frame. |
| No server dimension validation | MEDIUM | `grid.rs:74-83` | `apply_full_sync()` accepts server-provided cols/rows without bounds checking. `u16::MAX` dimensions could cause massive allocations. |
| Hard exit on connection loss | MEDIUM | `main.rs:1121-1127` | After max reconnect attempts, calls `event_loop.exit()` without warning. Should show "disconnected" state in UI. |
| Write channel backpressure | MEDIUM | `connection.rs:39-40` | Bounded channel (256) for outgoing messages. If server is slow, `send()` blocks the render loop, causing UI freeze. |
| Surface resize no minimum | LOW-MEDIUM | `main.rs:1243-1271` | Checks `width == 0 || height == 0` but no minimum size. Bar height subtraction could go negative. |

### Recommendation

**High priority**: replace `.expect()` with proper error handling. **Medium**: preserve scrollback on disconnect, implement idle-mode `ControlFlow::Wait`, add event processing budget per frame.

---

## 6. ciri-layout (Layout System)

### What it does right

- **Clean hierarchy**: `WorkspaceSet` → `Workspace` → `Column` → Tile (PaneId).
- **Flexible width model**: `ColumnWidth::Proportion(f64)` vs `ColumnWidth::Fixed(f32)`.
- **Viewport culling**: `visible_tiles()` and `visible_tiles_2d()` skip off-screen panes.
- **Camera clamping**: prevents blank space on right edge (`workspace.rs:64-65`).
- **Well-tested**: 11 unit tests across modules.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| Animation state split | MEDIUM | `column.rs:23` + main.rs | `Column.rendered_width` and `ViewOffset` both track width animation — double state management. Could desync. |
| Row-major only | Architectural | — | No vertical splits within a column. Simpler than i3/sway tree model but limits layout flexibility. Intentional constraint — document it. |
| No zero-viewport validation | LOW | `workspace.rs:186-201` | `resize_active_column` divides by `self.view_size.width` with implicit fallback to 0.5 if width <= 0. Should validate explicitly. |

---

## 7. ciri-config (Configuration)

### What it does right

- **Serde-based TOML** with `#[serde(default)]` for optional fields.
- **Preset theme system** with field-level override merging (`theme.rs:69-95`).
- **XDG-compliant** config path resolution on Unix.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| No semantic validation | HIGH | `config.rs:207-217` | `animation.speed`, `frame_interval_ms`, `font.size` never validated. Negative or zero values break rendering. Must validate at load time. |
| HOME panic | HIGH | `config.rs:228` | `.expect("neither XDG_CONFIG_HOME nor HOME is set")` — panics if HOME unset. Should return error or use fallback. |
| Silent color parse failure | MEDIUM | `theme.rs:55-65` | Invalid hex (`#GGGGGG`) silently becomes black via `unwrap_or(0)`. Should return error and log. |
| No hot-reload support | MEDIUM | — | Main app has `notify::Watcher` but ciri-config has no `reload()` function. Watcher detects changes but can't apply them. |
| KeybindConfig no validation | LOW | `keys.rs` | Typos in action names (e.g., `"nwe_column_right"`) silently become no-ops. Keybind.rs logs a warning, but error should surface to user. |

---

## 8. ciri-input (Input Handling)

### What it does right

- **Leader-key state machine** (Idle → AwaitingAction) with configurable timeout (1000ms default).
- **Double-tap detection** for SendLeaderKey (literal leader char passthrough).
- **Modifier support**: shift, ctrl, alt, super all tracked.
- **6 unit tests** including timeout and double-tap edge cases.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| Key name normalization | MEDIUM | `keybind.rs:49` vs `leader.rs:64` | `KeyCombo::parse()` lowercases key names, but `process_key()` compares directly. If event source provides "Space" but storage has "space", lookup fails silently. |
| Default leader_ctrl_key "w" | MEDIUM | `leader.rs:34` | Defaults to "w" but main.rs overwrites with config value. Hidden contract — if main.rs forgets, wrong key activates leader mode. |
| ScrollPageUp/Down unbound | LOW | `action.rs:32-35` | Actions defined in enum but never appear in default keybindings. Either bind or remove. |

---

## 9. ciri-anim (Animation)

### What it does right

- **Critically damped spring** with analytical solution `x(t) = target + (C1 + C2*t) * e^(-omega*t)` — no numerical integration, no accumulation errors. Industry best practice (used by Apple, modern design systems).
- **Frame-independent**: uses `dt` parameter, not frame count.
- **Velocity preservation**: re-targeting mid-animation preserves momentum (`animation.rs:48-50`).
- **Gesture support**: trackpad scroll with smooth snap-to-target.
- **Rigorous tests**: 9 unit tests including no-overshoot guarantee and monotonicity.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| Config epsilon ignored | MEDIUM | `spring.rs:26` | `AnimationConfig.epsilon` exists in config but is never passed to Spring constructor. Spring always uses hardcoded 0.1. |
| Gesture loses velocity | MEDIUM | `animation.rs:84-87` | `begin_gesture()` replaces state with `Gesture(value)`, discarding spring velocity. Feels jarring if user grabs during animation. |
| No upper bound in gesture | MEDIUM | `animation.rs:90-93` | `update_gesture()` clamps to `max(0)` but has no upper bound. Caller must clamp — undocumented requirement. |
| Easing functions unused | LOW | `easing.rs` | `ease_out_cubic`, `ease_out_expo`, `linear` defined but never called anywhere. Dead code. |

---

## 10. ciri-session (Session Persistence)

### What it does right

- **Atomic writes** via temp file + rename (`save.rs:24-28`).
- **Path traversal prevention**: validates session names against `/`, `\`, `..`, `\0`.
- **Graceful missing file**: `restore_session` returns `Ok(None)` for missing files.
- **JSON format**: human-readable, manually editable.

### Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| Single-row persistence | MEDIUM | `state.rs` | `SessionState` only captures active row's columns. Multi-workspace (multi-row) layout is lost on save/restore. |
| No schema version | MEDIUM | `restore.rs:12-13` | `serde_json::from_str` with no version field. Format changes will silently break deserialization. |
| No pane content | Architectural | `state.rs:19-24` | Only layout is saved (pane_id, cwd, title). Terminal content, scrollback, cursor, running processes are lost. Intentional for client-server model, but should be documented. |
| Fragile active indices | MEDIUM | `state.rs:8,14` | `active_column_idx` / `active_tile_idx` stored as absolute indices. If panes change before restore, indices are out of bounds. Should store pane_id instead. |
| weight field unused | LOW | `state.rs:21` | `SavedTile.weight: f32` is serialized but never read or written. Dead field. |

---

## 11. Overall System Design

### Architecture Diagram

```
┌─────────────────────────────────────┐
│            ciri (client)            │
│                                     │
│  winit event loop                   │
│    ├── Input → ciri-input           │
│    ├── Layout → ciri-layout         │
│    ├── Animation → ciri-anim        │
│    ├── Grid → ClientPaneGrid        │
│    └── Render → ciri-render (wgpu)  │
│                                     │
│  crossbeam ←→ server-io thread      │
└──────────────┬──────────────────────┘
               │ Unix socket / TCP
┌──────────────┴──────────────────────┐
│          ciri-server (daemon)       │
│                                     │
│  tokio async runtime                │
│    ├── Accept loop                  │
│    ├── Per-client reader/writer     │
│    └── 16ms tick loop:              │
│         ├── PTY drain               │
│         ├── alacritty_terminal      │
│         ├── Damage extraction       │
│         ├── Frame encoding          │
│         └── Session save            │
│                                     │
│  State: Pane[], Workspace, Clients  │
└─────────────────────────────────────┘
```

### Data Flow (Complete Path)

```
User keypress
  → winit KeyEvent
  → ciri-input: keybind lookup → Action
  → ClientMessage::Input(pane_id, bytes)
  → crossbeam channel → server-io thread
  → codec::encode → TCP/Unix write
  → Server: codec::read_frame → handle_message
  → Pane::write_to_pty(bytes)
  → PTY child process (shell)
  → PTY output (response bytes)
  → Reader thread: read(65KB) → sync_channel
  → Tick: Pane::process_pty_output
  → alacritty_terminal::Processor::advance
  → Term grid mutation
  → Damage extraction (changed line ranges)
  → DamageAccumulator per client
  → CellDelta / FullPaneSync encoding
  → mpsc channel → writer task → TCP/Unix write
  → Client: codec::read_frame → ServerEvent
  → crossbeam channel → main thread
  → ClientPaneGrid::apply_delta / apply_full_sync
  → TerminalView::build_view_from_grid
  → GlyphAtlas::ensure_char → GPU texture upload
  → RectPipeline + GlyphPipeline → wgpu::RenderPass
  → GPU present → pixels on screen
```

### Technology Stack Assessment

| Technology | Purpose | Verdict |
|------------|---------|---------|
| wgpu 23 | GPU rendering | Correct — cross-platform Vulkan/Metal/DX12/WebGPU |
| winit 0.30 | Event loop | Correct — industry standard |
| alacritty\_terminal 0.25 | VT parser + grid | Correct — battle-tested |
| tokio (full) | Async runtime | Correct — needed for socket I/O |
| swash | Font rasterization | Correct — modern, fast |
| rmp-serde | Message encoding | Acceptable — compact but not zero-copy. ROADMAP mentions custom binary |
| bytemuck | Zero-copy cells | Excellent — 14-byte PackedCell with cast\_slice |
| crossbeam-channel | Thread communication | Correct — bounded MPSC |
| portable-pty | PTY abstraction | Correct — cross-platform |
| notify | Config file watcher | Correct — standard approach |

### Crate Dependency Graph

```
ciri (client binary)
├── ciri-protocol   (shared: messages, codec, framing)
├── ciri-render     (client-only: wgpu, glyph atlas)
│   └── ciri-protocol
├── ciri-layout     (shared: workspace geometry)
├── ciri-anim       (client-only: spring animation)
├── ciri-input      (client-only: keybindings)
└── ciri-config     (shared: TOML config, themes)

ciri-server (server binary)
├── ciri-protocol
├── ciri-term       (server-only: PTY, alacritty_terminal)
├── ciri-layout
├── ciri-config
└── ciri-session    (server-only: save/restore)

No circular dependencies. Clean DAG. ✓
```

---

## 12. Missing Production Features

| Feature | Priority | Effort | Notes |
|---------|----------|--------|-------|
| IME input (CJK) | HIGH | Medium | winit provides Ime events; `ime_preedit_active` field exists but no handler |
| Sixel / Kitty image protocol | HIGH | Large | Needs VT parser extension, GPU texture management, protocol message |
| Text shaping (harfbuzz) | MEDIUM | Medium | Required for ligatures, combining marks, proper non-Latin rendering |
| Vertical tiling (split panes) | MEDIUM | Large | Layout engine redesign needed |
| Config hot-reload | MEDIUM | Small | Watcher exists; need to apply new config to keybinds, theme, font |
| Accessibility | MEDIUM | Large | Platform-specific APIs (MSAA, NSAccessibility, AT-SPI) |
| OSC 52 clipboard | MEDIUM | Small | Server → client clipboard write, app requests |
| Bell / notification | LOW | Small | Forward `Event::BellVisual` to client, show visual indicator |
| Color emoji | LOW | Medium | Need CBDT/COLR support in glyph atlas |

---

## 13. Priority Fix List

### Critical (Fix Immediately)

1. **Unix socket permissions** — `daemon.rs:422`: set socket to 0o700 after bind. Any local user can currently hijack sessions.
2. **Grid dimension DoS** — `codec.rs:441-442`: validate `cols * rows <= 100_000_000` in `decode_full_pane_sync`. Malicious peer can trigger OOM.
3. **RLE decode overflow** — `codec.rs:367-394`: check `cells.len() + count <= expected` before appending.

### High (Fix Before Production)

4. **Replace `.expect()` in init** — `main.rs:1157,1160`: return proper errors on GPU/window init failure.
5. **Config semantic validation** — validate `animation.speed > 0`, `frame_interval_ms > 0`, `font.size > 0`, etc. at load time.
6. **Config HOME panic** — `config.rs:228`: replace `.expect()` with graceful error or fallback.
7. **Panic-while-locked** — `daemon.rs:847-895`: scope state lock more narrowly so handle\_message panic doesn't deadlock cleanup.
8. **Client spawn limit** — `daemon.rs:725`: add `Semaphore` to cap concurrent client connections.
9. **Slow client detection** — count consecutive `try_send` failures; disconnect clients that fall too far behind.

### Medium (Before v1.0)

10. **Preserve scrollback on disconnect** — `main.rs:247`: don't `pane_grids.clear()` on disconnect; validate against server state on reconnect.
11. **Reduce lock hold time** — `daemon.rs:457-668`: snapshot damage data while locked, build frames after releasing lock.
12. **Idle render mode** — `main.rs:1046`: switch to `ControlFlow::Wait` when no animations or pending updates.
13. **Event processing budget** — `main.rs:177`: limit server event drain to time budget or count per frame to prevent input lag.
14. **Reader thread cleanup** — `pty.rs:160-173`: store `JoinHandle`, join with timeout in Drop.
15. **Remove unnecessary Arc** — `pane.rs:31`: Pane is never shared; remove `Arc` from `Arc<Mutex<Term>>`.
16. **Flow control** — implement generation-based ack protocol (field already exists as `last_acked_generation`).
17. **Session schema version** — add `version: u32` to `SessionState` for forward compatibility.
18. **Full workspace persistence** — save all rows in `SessionState`, not just active row.

### Low (Technical Debt)

19. **Wire Spring epsilon from config** — `spring.rs:26`: accept epsilon as constructor parameter.
20. **Remove dead easing functions** — `easing.rs`: unused code.
21. **Remove dead weight field** — `state.rs:21`: `SavedTile.weight` never read/written.
22. **Key name normalization** — ensure consistent lowercase throughout input pipeline.
23. **Windows named pipes** — replace TCP localhost with `\\.\pipe\ciri-*` for better security.
24. **Named color parse errors** — return `Result` instead of silently defaulting to black.
