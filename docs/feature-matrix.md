# loomtty 功能审计矩阵

> **本次重新核对：2026-06-09，对照 `dev` 分支实际代码逐条复核。**
>
> 上一版（2026-05-11）已明显过时——它把多个**已完成**的功能仍标为"没做"，又把一整块**未合并分支**上的功能当成已落地。本次以 `dev` HEAD 为准重做，给每条结论附当前 file:line 证据。
>
> ⚠️ **分支提醒**：`usage / AI 用量统计`（Claude/Codex OAuth 探测 + Lua format-usage）整套**不在 dev 上**，活在未合并的 `feat/usage-statusbar` 分支（commit 5342952）。本文按 dev 现状记为"未合入"，详见对应章节。

---

## 自上一版（5-11）以来的主要变化

| 功能 | 上一版结论 | dev 现状 | 证据 |
|---|---|---|---|
| Web client / 浏览器 attach | ❌ 没做 | **✅ 完整落地** | `loomtty web` 命令 + WS 网关 + SPA，见「网络/远程」与「Web UI」章 |
| `msg capture-pane` | ❌ 紧迫待做 | **✅ 已实现** | `cli.rs:157`（含 `--scrollback-rows`/`--join-wrapped`/`--preserve-trailing-spaces`） |
| `msg list-prompts` | ❌ 待做 | **✅ 已实现** | `cli.rs:186`，server `ipc.rs:275-301` |
| Jump-to-prompt（scroll 模式跳 prompt） | ❌ 数据有 UI 没做 | **✅ 已实现** | `[` / `]` 键，`keys.rs:90-91` → `action.rs:64-66` → `ipc.rs:275-301` |
| 设置面板（settings panel） | （未提） | **✅ 新增，~50 字段** | `app/ui/settings_panel/`（mod/schema/dispatch/hit） |
| 键位帮助浮层（help overlay） | （未提） | **✅ 新增** | `app/ui/help_overlay.rs`，`Action::ToggleHelp` |
| 主题预设数 | 8 | **9** | `theme.rs:237-249`（补 `one_half_dark`） |
| usage / Claude·Codex 用量 | ✅ 当成已做 | **❌ 不在 dev**（在 `feat/usage-statusbar` 分支） | dev 上无 `usage/` 目录、无 `builtin/usage.lua` |

---

## 核心能力矩阵

### 1. CLI 命令（`crates/loom/src/cli.rs`）

| 命令 | 别名 | 状态 | 备注 |
|---|---|---|---|
| `loomtty`（无参） | — | ✅ | 自动 attach 最近 session 或新建 |
| `loomtty <name>` | — | ✅ | 按名 attach/创建 |
| `loomtty new` | — | ✅ | 强制新建 |
| `loomtty init` | — | ✅ | 交互式配置向导 `init::run_init()`（`init.rs:6`，`main.rs:37`） |
| `loomtty attach` | `a` | ✅ | 不存在则报错 |
| `loomtty list` | `ls` | ✅ | `-a` 含已保存 |
| `loomtty kill` | `k` | ✅ | |
| `loomtty kill-server` | `ks` | ✅ | |
| `loomtty delete` | `rm` | ✅ | 删除已保存 session |
| `loomtty remote HOST` | — | ✅ | 二进制协议 + SSH 隧道 |
| `loomtty msg <sub>` | — | ✅ **10 个子命令** | 见下表（`cli.rs:126-190`） |
| `loomtty tpl <sub>` | `template` | ✅ 3 个子命令 | ls/save/apply（`cli.rs:192-207`） |
| `loomtty web` | — | ✅ **新** | 浏览器终端网关（`cli.rs:100-123`，`web.rs`） |

### 2. IPC msg 子命令（`crates/loom-server/src/daemon/server/ipc.rs`）

| 子命令 | 实现 | 备注 |
|---|---|---|
| `send-keys` | ✅ | |
| `list-panes` | ⚠️ | 完整，但 **`cwd` 字段硬编码 `None`**（`ipc.rs:441`，`collect_pane_details`） |
| `info` | ✅ | session 详情 |
| `focus-pane` | ✅ | 含 agent diff 广播 |
| `close-pane` | ✅ | |
| `create-pane` | ✅ | |
| `get-layout` | ✅ | 返回完整 LayoutState |
| `run-command` | ⚠️ | 协议有 `cwd`，**CLI 没暴露**（`main.rs:95` 硬编码 `None`） |
| `capture-pane` | ✅ **新** | 抓 pane 文本；`--scrollback-rows N` / `--join-wrapped` / `--preserve-trailing-spaces` / `--json` |
| `list-prompts` | ✅ **新** | OSC 133 命令历史；返回 `PromptMarkInfo{prompt_line,output_line,done_line,exit_code,duration_ms}` |

**仍缺**：`pipe-pane`、`rename` / `rename-session`、把 `switch-workspace`/`switch-session`/`resize-pane`/`set-col-width` 暴露成 CLI（这些只有内部 ClientMessage，无 CLI 入口）。

### 3. Keybind Actions

| 类别 | Action | 默认绑定 | 文档化 |
|---|---|---|---|
| Navigation | FocusLeft/Right/Up/Down | Leader h/l/k/j | ✅ |
| Pane mgmt | CreatePane / SplitDown / ClosePane | Leader n / d / x | ✅ |
| Pane mgmt | MovePaneLeft/Right | Leader Shift+H/L | ✅ |
| Pane mgmt | ConsumeIntoColumn / ExpelFromColumn | Leader c / e | ✅ |
| Column width | ColumnWidthFull | Leader f | ✅ |
| Column width | OneThird/Half/TwoThirds | （未默认绑定） | ❌ 隐藏 |
| Column width | Increase/Decrease/Cycle/Equalize | resize 模式 h/l/r/= | ✅ |
| Tile height | TileHeightIncrease/Decrease | （未默认绑定） | ❌ 隐藏 |
| Scroll mode | jump-to-prompt PrevPrompt/NextPrompt | scroll 模式 `[` / `]` | ✅ **新** |
| Modes | EnterMode (scroll/move/resize) | Leader s/m/r | ✅ |
| Modes | ToggleBroadcast/Lock/Overview/Palette | Leader b / Ctrl+G / Tab / p | ✅ |
| Modes | ToggleSettings / ToggleHelp | （面板/浮层） | ✅ **新** |
| Modes | ToggleSessionPalette | Leader Shift+p | ❌ 隐藏 |
| Sessions | NextSession/PrevSession/NewSession | Leader i / Shift+i / Shift+n | ❌ 隐藏 |
| Workspace | SwitchWorkspace(0..8) | Leader 1..9 | ❌ 隐藏 |
| Scroll mode | LineUp/Down PageUp/Down HalfPage Top/Bottom | j/k/u/d/f/b/g/G | ✅ |
| Direct | OpenSearch / Copy / Paste | Ctrl+Shift+F / C / V | ✅ |
| Other | Detach | Leader q | ✅ |
| Other | SendLeaderKey | 双击 Leader | ❌ 隐藏 |

---

## 终端协议 / 渲染功能（`crates/loom-term/src/`）

| 功能 | 状态 | 实现位置 |
|---|---|---|
| OSC 7 (cwd) | ✅ | `osc7_parser.rs`（仅接受本地 hostname，防注入） |
| OSC 8 (hyperlinks) | ✅ 4096 link map | `osc8_parser.rs` |
| OSC 9 (iTerm2/ConEmu notify) | ✅ | `pane/notify.rs`（区分 iTerm2 / ConEmu，限速 1s） |
| OSC 133 (prompt marks A/B/C/D) | ✅ 含 exit_code | `shell_integration.rs:14-45` |
| OSC 777 (xterm notify ext) | ✅ | `pane/notify.rs` |
| Kitty graphics protocol | ✅ ~400 行 | `kitty_graphics.rs`（`parser_suite::scan_images`） |
| Sixel | ✅ ~600 行 | `sixel.rs` |
| Inline image rendering | ✅ | `ServerMessage::ImagePlacement` + render |
| Kitty keyboard protocol | ✅ Level 1-5 | `loom/src/app/key_encode/kitty.rs` |
| Legacy key encoding | ✅ | `key_encode/legacy.rs` |
| Ligatures（含 fi / Arabic 合字） | ✅ 带缓存 | `loom-render/src/shaper.rs`；macOS 走 `shaper_coretext.rs` |
| 字体回退（cmap / DirectWrite） | ✅ | `loom-render/src/font_resolver/` |
| IME / CJK | ✅ | `loom/src/app/ime.rs` |
| 预测回显（mosh 风） | ✅ 三档 Off/Always/Adaptive | `loom-app/src/prediction/`（SRTT 自适应 + 下划线 overlay） |
| 滚动条 | ✅ | `loom-render/src/terminal/scrollbar.rs` |
| Bell（视觉+音频） | ✅ | `app/ui/bell_flash.rs`，`notification.rs` 内 audio |

---

## GPU 渲染 / 视觉

| 功能 | 状态 | 备注 |
|---|---|---|
| Blade 后端（Vulkan/Metal） | ✅ | `loom-gpu/src/blade.rs`（~2.1K 行） |
| OpenGL 后端 | ✅ | `gl.rs`（~2.4K 行，平滑 Wayland resize） |
| DX11 后端 | ✅ | `dx.rs`（~2.2K 行，Windows） |
| 后端自动选择 | ✅ | `render.backend = "auto"` → blade，回退 gl |
| 分层渲染管线 | ✅ | render-pipeline 重构已在 dev |
| Glyph atlas + alpha/color | ✅ | `loom-render/src/glyph_cache/` |
| SDF 圆角 pane | ✅ | `loom-render/src/sdf_rect.rs` |
| 焦点环 SDF（Solid/Glow） | ✅ | `render.rs:126` `focus_ring_uses_sdf()`，`emit_focus_ring_sdf()` |
| 全局壁纸 / 背景图 | ✅ | `app/background_image.rs`（异步解码） |
| Pane 不透明度 / 失焦半透 | ✅ | `pane_opacity` / `inactive_opacity` |
| 动画（bezier / spring / easing） | ✅ 4 preset | `crates/loom-anim/` |
| 虚线焦点环边框 | ⚠️ 矩形段实现，SDF 版未做 | `emit_dashed_border()`（`render.rs:3107`）走分段矩形；SDF 版仍 TODO（`render.rs:452,1694`） |
| 自定义 fragment shader hook | ❌ | 未暴露 |

---

## Multiplexer 核心

| 功能 | 状态 | 备注 |
|---|---|---|
| Session detach/attach | ✅ | 黄金标准 |
| 列布局 + workspace | ✅ | `loom-layout/`（column / workspace / tile） |
| 模板（template）保存/加载 | ✅ | `loom-session/src/template.rs` + CLI |
| Session 持久化（agent-aware） | ✅ | claude / codex / opencode / droid 自动 resume（`loom-session/src/agent.rs:52-85`） |
| 多 client 共享 session（基础） | ✅ server 层 | `broadcast_to_session`；UI 无"邀请/加入"流程 |
| 浮动 pane（zellij 招牌） | ❌ | layout crate 无实现 |
| Stack 布局（zellij 招牌） | ❌ | 无独立 stack 类型（列内 tile 纵向堆叠是另一回事） |
| Workspace 跨多列 | ✅ | |
| 输入模式：prefix vs sticky | ✅ | `loom-input/src/leader/handler.rs` |
| 广播输入到全部 pane | ✅ | `core.broadcast_mode`（密码输入时抑制） |
| Lock 模式（透传所有键） | ✅ | `Ctrl+G` |
| Overview 模式（缩略图） | ✅ | `app/overview.rs` + `Leader Tab` |
| 命令 palette | ✅ | `app/ui/palette.rs` + `Leader p` |
| Session palette | ✅ | `Leader Shift+p` |
| 设置面板 | ✅ **新** | `app/ui/settings_panel/`，~50 字段、可滚动 |
| 键位帮助浮层 | ✅ **新** | `app/ui/help_overlay.rs`，数据驱动多列布局 |
| 搜索 | ⚠️ 字面 + 大小写不敏感 | `loom-app/src/grid/cell_ops.rs:241`（强制 `to_lowercase`）；无正则、无大小写切换 |
| 鼠标拖拽调列宽/行高 | ✅ | `app/mouse.rs`（column + tile resize drag） |
| Focus follows mouse | ✅ | 配置项 + 设置面板字段 |
| Copy-on-select | ✅ | 配置项 + 设置面板字段 |
| 右键上下文菜单 | ✅ | `app/ui/context_menu.rs`（Copy/Paste/SelectAll/Open link） |
| Tab 条（横向集成 + 纵向侧栏） | ✅ | `ui/top_bar/pane_tabs.rs` + `ui/tab_bar/`（Integrated/Left/Right） |
| 键位提示条（chrome hints bar） | ✅ | `ui/hints_bar.rs`（显示可用键位提示，**非** kitty URL 拾取器） |
| 命名 paste buffer（tmux 风） | ❌ | |

---

## Shell 集成

| 功能 | 状态 | 备注 |
|---|---|---|
| bash / zsh / fish 集成脚本 | ✅ | `loom-server/shell-integration/loom.{bash,zsh,fish}` |
| OSC 133 解析（A/B/C/D） | ✅ | `shell_integration.rs:14-45`，含 exit_code |
| Jump-to-prompt（scroll 模式 `[`/`]`） | ✅ **新** | `keys.rs:90-91` → `PrevPrompt/NextPrompt` → server `JumpToPrompt`（`ipc.rs:275-301`） |
| 命令耗时（duration） | ⚠️ 采集到，渲染未落地 | `PromptMark` 存 duration；仅用于完成通知（`sync.rs:339-354`），未在 grid 显示 |
| Exit code | ⚠️ 采集到，渲染未落地 | 存于 `PromptMark.exit_code`、传到 client、用于通知；**grid 不按 exit code 染色** |
| 滚动条按 prompt 染色 | ❌ | |
| `msg list-prompts` 查询 | ✅ | 见 IPC 表 |

---

## 网络 / 远程

| 功能 | 状态 | 备注 |
|---|---|---|
| 二进制 RDP 风协议 | ✅ | `loom-protocol/` |
| 远程 attach 探测既有 sessions | ✅ | `connection::probe_remote_sessions_blocking` |
| 远程 host 记忆（recent_hosts） | ✅ | |
| SSH 隧道 | ✅ | |
| Cell delta 增量编码 / full sync | ✅ | |
| 协议 state machine + benches | ✅ | `bench/protocol_decode.rs` |
| **Web client / 浏览器 attach** | **✅ 完整**（上版误判为 ❌） | 见下「Web UI」章 |
| 多用户多光标协作（zellij 风） | ❌ | server 框架有，UI 没暴露 |
| Mosh 风 UDP / QUIC | ❌ | 现走 TCP |

### Web UI（`feat/web-product` 已并入 dev）

| 子项 | 状态 | 位置 / 证据 |
|---|---|---|
| `loomtty web` 命令 | ✅ | `cli.rs:100-123`（`--port/--bind/--token/--static-dir/--open`），`web.rs` |
| HTTP + WS 单端口网关 | ✅ | `loom-server/src/daemon/web.rs`（`GET /ws` 升级，其余走 `ServeDir`） |
| Token 鉴权（≥16B，drop 时 zeroize） | ✅ | `web.rs:106-123` |
| Origin allowlist（非 loopback 强制） | ✅ | `web.rs:311-327`（防 CSRF） |
| TLS 姿态 | 明文 http/ws，前置反代终止 TLS | `web/README.md` 明确说明 |
| 浏览器 SPA（msgpack codec + WS 传输 + DOM 渲染） | ✅ | `web/packages/`：loom-codec / loom-client / loom-dom / loom-app / loom-web |
| sessions / workspace 切换 chrome | ✅ | |
| IME + preedit overlay | ✅ | |
| 内联图像（sixel/kitty/iTerm） | ✅ | |
| 搜索 / 复制粘贴（原生事件 + Clipboard 回退） | ✅ | |
| 触屏 / 长按选择 / safe-area | ✅ | |
| 可安装 PWA（manifest + service worker） | ✅ | |
| 裸 URL 自动链接 | ✅ | `web/packages/loom-dom/src/linkify.ts` |
| 通知 toast（Notification / SessionKilled / CommandCompleted） | ✅ | |
| 回到底部按钮 | ✅ | |

---

## 插件 / 扩展（`crates/loom-plugin/src/`）

| 功能 | 状态 | 备注 |
|---|---|---|
| Lua VM | ✅ | `vm.rs`（built-in → 用户插件目录 → 用户 init.lua） |
| 沙箱 | ✅ | `sandbox.rs`（白名单 stdlib，10 万指令上限/回调） |
| 事件系统（失败计数） | ✅ | `events.rs`，连续 3 次失败自动禁用 handler |
| 内建插件：session_restore | ✅ | `builtin/session_restore.lua`（agent 检测，唯一内建） |
| ~~内建插件：usage~~ | ❌ **dev 上不存在** | 上一版误记；`builtin/` 只有 `session_restore.lua`，usage 在 `feat/usage-statusbar` 分支 |
| Lua API | ⚠️ | `api.rs`：`loom.on` / `loom.log` / `loom.warn`，仅内联注释、无独立文档 |
| 插件 install / 包管理 / marketplace | ❌ | 现状只能手动放到 `~/.config/loom/plugins/<name>/init.lua` |

---

## 平台支持

| 功能 | macOS | Linux | Windows |
|---|---|---|---|
| GUI 启动 | ✅ | ✅ | ✅ |
| GPU 后端（自动） | Blade(Metal) | Blade(Vulkan) / GL | DX11 |
| PTY | ✅ | ✅ | ✅ ConPTY |
| 系统托盘 | ✅ | ✅ | ✅ |
| 自启动 | ✅ launchd | ✅ .desktop | ✅ HKCU Run 键 |
| 桌面通知 | ✅ notify_rust | ✅ notify_rust | ✅ WinRT toast（notify-rust → tauri-winrt-notification） |
| Procinfo（前台进程） | ✅ proc_pidinfo | ✅ /proc/PID | ✅ 进程树 + PEB 读 argv/cwd |
| Agent 检测（依赖 argv） | ✅ | ✅ | ✅ |

> Windows procinfo：`windows.rs` 进程快照 + 进程树遍历；argv/cwd 通过 `NtQueryInformationProcess` + `ReadProcessMemory` 读目标进程 PEB（`read_peb_strings`，手定 x64 偏移 + 奇数长度校验）。解锁了 Win 上 agent 自动恢复（识别 `codex exec` 等需要 argv）与 cwd 探测。

---

## 用量统计 / AI 集成

> ⚠️ **整章在 dev 上未合入**：以下功能位于 `feat/usage-statusbar` 分支（commit 5342952 "feat(usage): Claude/Codex OAuth probe with Lua format-usage plugin"），dev HEAD 上**没有** `crates/loom/src/app/usage/`、也没有 `builtin/usage.lua`。上一版把它当成已落地，是误记。

| 功能 | dev 状态 | 备注 |
|---|---|---|
| Claude / Codex OAuth 探测、token 管理、poller、snapshot | ❌ 未合入 | 在 `feat/usage-statusbar` 分支 |
| Lua 自定义 format（按 pane 切换） | ❌ 未合入 | 同上 |
| 状态栏用量展示 | ❌ 未合入 | dev 状态栏 `app/status_bar.rs` 不含用量段 |
| Agent 检测 / 自动 resume | ✅（dev 已有） | `loom-session/src/agent.rs`，与 usage 无关 |
| AI inline 解释（选中 → 解释） | ❌ | |
| Scrollback 摘要 | ❌ | |
| Agent attach pane（专门 pane 类型） | ❌ | |
| 命令 ghost-text 建议 | ❌ | |

---

## 配置 / 主题（`crates/loom-config/src/`）

| 功能 | 状态 | 备注 |
|---|---|---|
| TOML 配置 | ✅ | `~/.config/loom/config.toml` |
| 配置热重载 | ✅ | `app/event.rs` 监听配置**目录**（容忍原子 rename）+ 自写回声抑制 + 去抖 |
| 配置校验 | ✅ | garde schema 校验 |
| Init wizard | ✅ | `loomtty init` → `init::run_init()`（`init.rs:6`） |
| 设置面板（运行时改配置） | ✅ **新** | `app/ui/settings_panel/`，~50 字段 9 大类 |
| 主题预设 | ✅ **9 个** | loom_dark / one_dark / one_half_dark / catppuccin_mocha / tokyo_night / dracula / nord / gruvbox_dark / ghostty（`theme.rs:237-249`） |
| 主题自定义（TOML 覆盖） | ✅ | `ThemeValue` 跟踪 explicitly_set |
| 导入 Alacritty / iTerm2 / tmux.conf / zellij.kdl | ❌ | 无任何 importer |
| 多 profile（多 shell 配置） | ❌ | |
| 项目级 auto-layout（按 cwd） | ❌ | 模板有，cwd 触发器无 |

---

## 半成品 / 已知缺陷（dev）

| 项 | 文件:行 | 影响 |
|---|---|---|
| `list-panes` 的 `cwd` 永远 `None` | `daemon/server/ipc.rs:441` | JSON/CLI 输出 cwd 始终为空 |
| `run-command` 不能传 `--cwd` | `main.rs:95` | 协议有字段，CLI 没暴露 |
| 虚线焦点环只有矩形段、SDF 版未做 | `render.rs` | 视觉细节 |
| 搜索无正则 / 无大小写切换 | `loom-app/src/grid/cell_ops.rs:241` | 仅字面、强制小写匹配 |
| 命令耗时 / exit code 未在 grid 渲染 | `sync.rs` 仅用于通知 | OSC 133 数据采集到但 UI 没全用上 |
| 多 client UI 未暴露 | server 层已就绪 | 没有"分享 session"用户流程 |
| usage 用量功能未合入 dev | `feat/usage-statusbar` 分支 | 状态栏无用量展示 |

---

## 完全没做（按战略价值排）

### 🔴 紧迫（AI / 多路复用核心方向）

| 项 | 价值 |
|---|---|
| `msg pipe-pane`（流式输出 tee） | 持续日志 / AI 实时分析 |
| AI inline 解释（选中 → 流式答复到下个 pane） | 独家差异化 |
| Agent attach pane（pane 类型） | 把 Claude Code / Codex 一等公民化 |
| 浮动 pane | 抢 zellij 招牌；lazygit/gh dash 类高频 |
| Stack 布局 | "列 + stack" 是独家组合形态 |
| **把 usage 分支合入 dev** | OAuth 探测已写好，只差合并 + 收尾 |

> 注：`msg capture-pane`、Web client、jump-to-prompt 已从本清单**毕业**（已实现）。

### 🟡 中等（用户体验 / 生态）

| 项 | 价值 |
|---|---|
| Shell 集成 UX 落地渲染（exit code 染色、耗时显示、滚动条 prompt 标记） | OSC 133 数据已采集，差 grid 渲染 |
| Hints / URL 键盘 picker（kitty 风） | 对 leader 体系是天然延伸（现 `hints_bar` 只是提示条） |
| 命名 paste buffer | tmux 用户日用 |
| `rename-session` / `msg rename` | tmux 等价 |
| 协议有 / CLI 缺：`switch-workspace` / `switch-session` / `resize-pane` / `set-col-width` | 一个下午能补齐 |

### 🟢 长期（差异化 / 出圈）

| 项 | 价值 |
|---|---|
| 多 cursor 协作（zellij 招牌） | 配对编程；server 框架已有 |
| 配置导入器（tmux/zellij/iterm/alacritty） | 拉存量用户最便宜 |
| 插件 marketplace + `loomtty plugin install` | 从工具变平台 |
| 自定义 fragment shader hook | Ghostty 出圈点 |
| Quake / 下拉模式 + 全局热键 | Ghostty / WT 都有 |
| Mosh 风 UDP / QUIC 传输层 | 网络抖动 / roaming |
| 多 profile（多 shell 一窗） | Win 用户首要诉求 |
| 搜索正则 / 大小写 toggle | UX 改进 |

### 🔵 不必追

| 项 | 原因 |
|---|---|
| ssh-kitten 等价（terminfo copy） | 有自有二进制协议，不必走 SSH 套壳 |
| GUI 配置器 | 已有运行时设置面板 + toml |
| 跑在任意 host 终端内 | 结构性差异，不要倒退 |

---

## 与主流终端横向对比

| 维度 | tmux | zellij | Ghostty | Kitty | WT | **loomtty** |
|---|---|---|---|---|---|---|
| 内建 multiplexer | ✅ | ✅ | ❌ | △ | ❌ | ✅ 列+workspace |
| 网络远程 attach | ❌ | ❌ | ❌ | △ | ❌ | ✅ 二进制 |
| 自有 GPU 渲染 | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ 三后端 |
| Win 原生 | ❌ | ❌ | ⏳ | ❌ | ✅ | ✅ |
| 预测回显 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| 配置热重载 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 桌面通知 OSC 9 | ❌ | △ | ✅ | ✅ | △ | ✅ 全平台 |
| Kitty graphics | ❌ | △ | ✅ | ✅ | ❌ | ✅ |
| Sixel | △ | △ | ✅ | ✅ | △ | ✅ |
| 浮动 pane | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Stack 布局 | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Hints/URL picker | ❌ | △ | ✅ | ✅ | ❌ | ❌ |
| Quake 模式 | ❌ | ❌ | ✅ | ✅ | ✅ | ❌ |
| 多 profile | ❌ | △ | △ | △ | ✅ | ❌ |
| 多 cursor 协作 | △ | ✅ | ❌ | ❌ | ❌ | △ 框架有，UI 无 |
| Web client | ❌ | ✅ | ❌ | ❌ | ❌ | **✅**（已落地） |
| 插件 marketplace | ✅ TPM | △ | ❌ | ✅ | ❌ | ❌（有 Lua 沙箱，无包管理） |
| Sticky 输入模式 | ❌ | ✅ | ❌ | ❌ | ❌ | ✅ |
| Tab 条 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ 横向+纵向 |
| Jump-to-prompt | ✅ | △ | ✅ | ✅ | ❌ | **✅** `[`/`]` |
| Agent-aware session restore | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ 独家 |
| 内联图像 | ❌ | △ | ✅ | ✅ | ❌ | ✅ |
| Ligature | △ | △ | ✅ | ✅ | ✅ | ✅ |
| 主题预设数 | 社区 | 社区 | 250+ | 社区 | 社区 | 9 内置 |

---

## TL;DR

dev 分支当前真实形态：

**已是真正的独家武器**：三 GPU 后端、列+workspace 布局、二进制远程协议、预测回显、agent-aware session restore、内联图像、**完整的 Web 浏览器客户端**（含 PWA）、Lua 沙箱插件。

**自 5-11 以来新落地**：Web client、`msg capture-pane`、`msg list-prompts`、jump-to-prompt（`[`/`]`）、运行时设置面板（~50 字段）、键位帮助浮层、主题增到 9 个、Windows 桌面通知（WinRT toast）、Windows procinfo PEB 读取（argv/cwd → 解锁 Win 上 agent 恢复）。

**真正还没做的短板**：
1. **Shell 集成数据没全落地 UI**——exit code 染色 / 耗时显示 / 滚动条 prompt 标记。
2. **usage 用量功能没合入 dev**——OAuth 探测写好了，躺在 `feat/usage-statusbar` 分支。
3. **zellij 招牌没拿**——浮动 pane / stack / 多 cursor 协作。
4. **配置生态封闭**——无导入器、无插件包管理、无多 profile。

**建议下一步**：把 usage 分支（`feat/usage-statusbar`）合入 dev——OAuth 探测已写好，只差合并 + 收尾；其次把 Shell 集成数据落地 grid 渲染（exit code 染色 / 耗时显示）。
