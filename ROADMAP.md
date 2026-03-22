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
- [x] Sixel image protocol — full decoder, RGBA conversion, inline placement
- [x] **Kitty keyboard protocol** — CSI u format with per-pane detection
- [x] **OSC 8 hyperlinks** — inline hyperlink parsing, per-cell link ID mapping
- [x] **DEC private modes** — focus events (1004), synchronized output (2026)

## Session & Templates

- [x] 2D session state: saves all workspaces
- [x] Atomic writes (temp file + rename)
- [x] Session restore on daemon startup
- [x] Delete stale session on clean exit
- [x] Session name validation
- [x] Scrollback included in attach (up to 1000 lines)
- [x] Auto-save on layout changes (debounced 250ms)
- [x] **Layout templates** — TOML files defining startup layouts, `ciri template apply/save/list`
- [ ] Schema versioning for forward compatibility
- [ ] Template variables — `$CWD`, `$PROJECT` in command/cwd fields

## Ecosystem

- [x] **IPC / CLI interface** — `ciri msg` commands for scripting, `--json` output
- [x] **Remote session** — SSH tunnel via `ssh -W`, server TCP listener, `ciri remote` command
- [ ] **Command palette** (fuzzy finder) — search panes by title/CWD/command, switch sessions, execute actions
- [ ] **Niri native integration** — leverage niri IPC when running under niri compositor

## Platform

- [x] Windows: named pipes (`\\.\pipe\ciri-server`) replacing TCP localhost
- [ ] macOS: test and fix Unix domain socket path handling
- [ ] Background opacity / blur

---

## Next: Shell & UX (路线 A — 补齐体验短板)

让从 tmux/Ghostty 迁移的用户感到舒适。

### A1: Shell Integration 自动注入

- [ ] **Bash integration script** — 通过 `PROMPT_COMMAND` 注入 OSC 133 标记 + CWD 上报 (OSC 7)
- [ ] **Zsh integration script** — 通过 `precmd`/`preexec` hooks 注入
- [ ] **Fish integration script** — 通过 `fish_prompt`/`fish_preexec` functions 注入
- [ ] **自动注入机制** — 服务端启动 PTY 时设置环境变量 (`CIRI_SHELL_INTEGRATION_DIR`)，shell rc 文件自动 source
- [ ] **CWD 追踪** — 新面板继承当前面板的工作目录（通过 OSC 7 上报的 CWD）
- [ ] **命令完成通知** — shell 报告命令退出码 (OSC 133;D)，长时间命令结束时发送桌面通知
- [ ] **命令耗时显示** — 状态栏显示上一条命令的执行时长

### A2: 粘贴保护 & 选择增强

- [ ] **粘贴保护** — 检测粘贴内容是否含危险模式 (`sudo rm`, `curl | sh`, 多行命令)，弹出确认
- [ ] **Copy-on-select** — 选中文本自动复制到剪贴板，可配置目标 (clipboard / primary selection)
- [ ] **选中即取消** — 输入任意字符取消当前选择（可配置）
- [ ] **选择词边界自定义** — `[terminal] word_delimiters` 配置项

### A3: Focus follows mouse

- [ ] **鼠标聚焦** — 鼠标移入面板自动切换焦点（`[input] focus_follows_mouse = true`）
- [ ] **可配置延迟** — 防止快速移动鼠标时误切换（`focus_follows_mouse_delay_ms`）

### A4: 通知增强

- [ ] **桌面通知** — 长时间命令完成时发送系统通知 (需 shell integration A1)
- [ ] **通知阈值** — `[terminal] notify_after_seconds = 10`（命令超过 N 秒才通知）
- [ ] **Bell 音频** — 可配置音频文件播放（`[terminal] bell_audio = "path/to/sound.wav"`）
- [ ] **窗口紧急提示** — Bell 时 `_NET_WM_STATE_DEMANDS_ATTENTION`

### A5: 右键上下文菜单

- [ ] **上下文菜单** — 右键弹出菜单：复制 / 粘贴 / 搜索 / 打开链接 / 面板操作
- [ ] **链接右键** — 在链接上右键显示「复制链接」「在浏览器打开」
- [ ] **选中文本右键** — 「复制」「搜索」「在新面板运行」

---

## Future: 差异化扩展 (路线 B — Ghostty 没有的)

Ciri 独有的终端复用能力，扩大与 Ghostty/tmux/zellij 的差距。

### B1: Plugin 系统

- [ ] **Lua 插件运行时** — 嵌入 mlua，插件可访问面板状态/布局/配置
- [ ] **插件 API** — `ciri.on_pane_created`, `ciri.on_command_finished`, `ciri.on_layout_changed` 等事件钩子
- [ ] **自定义状态栏模块** — 插件可注册状态栏 widget（git branch、k8s context、天气等）
- [ ] **自定义 Action** — 插件注册新的 Action 和快捷键
- [ ] **插件管理** — `ciri plugin install/list/remove`，从 Git URL 安装
- [ ] **沙箱隔离** — 插件运行在受限环境，不可直接执行 shell 命令（除非声明权限）

### B2: 键序列 & Key Table

- [ ] **多步键序列** — 支持 `Leader + a > n`（按 a 进入子 table，再按 n 触发动作）
- [ ] **命名 Key Table** — `[keys.resize_mode]` 定义 resize 专用键表，进入后 h/l/j/k 调整尺寸，Esc 退出
- [ ] **Key Table 超时** — 子 table 可配置自动退出时间
- [ ] **状态栏指示** — 当前活跃 key table 名称显示在状态栏

### B3: 自定义 Shader

- [ ] **Fragment shader 管线** — 终端渲染结果作为 texture 输入，用户 GLSL shader 后处理
- [ ] **Shadertoy 兼容 uniforms** — `iResolution`, `iTime`, `iFrame`, `iChannel0` (终端纹理)
- [ ] **Ciri 专属 uniforms** — `iCursorPos`, `iCursorColor`, `iFocused`, `iPalette[16]`
- [ ] **Shader 热重载** — 修改 .glsl 文件自动重新编译
- [ ] **内置 shader 库** — CRT 效果、模糊、色调映射、扫描线等预置 shader
- [ ] **配置** — `[render] shader = "path/to/shader.glsl"` 或 `shader = "crt"`

### B4: 渲染质量提升

- [ ] **Variable font 支持** — 可配置 OpenType 变量轴（weight, width, slant, ital）
- [ ] **字体特性控制** — `[font] features = ["liga", "calt", "-ss01"]`（启用/禁用 OpenType features）
- [ ] **字体度量微调** — `cell_width_scale`, `baseline_offset`, `underline_position`, `underline_thickness`
- [ ] **Nerd Font 检测** — 自动识别 Nerd Font 并调整图标字形渲染
- [ ] **最小对比度** — `[appearance] minimum_contrast = 4.5`（WCAG 2.0 AA 标准）
- [ ] **亚像素渲染控制** — FreeType hinting 模式可配（none / light / full）
- [ ] **颜色空间** — 支持 Display P3 (macOS) 和线性混合

### B5: Quick Terminal

- [ ] **全局热键** — 系统级快捷键呼出/隐藏浮层终端（如 Guake/Yakuake）
- [ ] **可配置位置** — top / bottom / left / right / center
- [ ] **可配置大小** — 百分比或像素
- [ ] **动画** — 滑入/淡入动画
- [ ] **自动隐藏** — 失焦后自动收起
- [ ] **独立会话** — Quick Terminal 绑定到专用会话

### B6: Undo / Redo

- [ ] **面板恢复** — 关闭面板后可撤销（保留 PTY 一段时间再真正销毁）
- [ ] **撤销超时** — 可配置保留时间（默认 5 秒）
- [ ] **操作历史** — 记录最近 N 次布局操作，支持多步撤销
