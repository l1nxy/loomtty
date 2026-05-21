# ciritty 功能审计矩阵

> 截至 2026-05-11，对照本仓库实际代码核对。
> 之前我在对话里多次断言某些功能"没有做"，但**多个被我误判**——这份文档把所有结论以代码引用作为依据重做一遍。

---

## 修正之前的错判

| 之前说"没有" | 实际状态 | 证据 |
|---|---|---|
| 配置热重载 | **✅ 完整实现** | `crates/ciri/src/app/event.rs:443-465` — `notify::recommended_watcher` + crossbeam channel + `reload_config()` |
| 桌面通知（OSC 9 / 99） | **✅ Unix 完整，Win 是 stub** | `crates/ciri-term/src/pane/notify.rs:94,97` 解析 OSC 9/777；`crates/ciri/src/app/notification.rs:25` 用 `notify_rust` 发送；`#[cfg(not(unix))]` 分支为空 |
| 标签页 | **✅ 横向 + 纵向两种** | `crates/ciri/src/app/ui/tab_bar/mod.rs`（纵向左/右），`top_bar/pane_tabs.rs`（横向集成） |
| 命令完成时通知（含 exit code） | **✅ 已接通** | `crates/ciri/src/app/sync.rs:224-253` — `ServerMessage::CommandCompleted { exit_code }` 触发通知 |
| Sticky 输入模式（zellij 风） | **✅ 已实现** | `crates/ciri-input/src/leader/handler.rs:406` `InputMode::Sticky` |
| 多 client 共享 session 基础设施 | **✅ server 层已具备** | `daemon/server/mod.rs:132` `broadcast_to_session`；`tests.rs:1064` shared-session 测试 |

---

## 核心能力矩阵

### 1. CLI 命令（全部已实现）

| 命令 | 别名 | 状态 | 备注 |
|---|---|---|---|
| `ciritty` (无参) | — | ✅ | 自动 attach 最近 session 或新建 |
| `ciritty <name>` | — | ✅ | 按名 attach/创建 |
| `ciritty new` | — | ✅ | 强制创建新 session |
| `ciritty init` | — | ✅ | 交互式配置向导 |
| `ciritty attach` | `a` | ✅ | 不存在则报错 |
| `ciritty list` | `ls` | ✅ `-a` 含已保存 |
| `ciritty kill` | `k` | ✅ | |
| `ciritty kill-server` | `ks` | ✅ | |
| `ciritty delete` | `rm` | ✅ | 删除已保存 session |
| `ciritty remote HOST` | — | ✅ | 二进制协议 + SSH 隧道 |
| `ciritty msg <sub>` | — | ✅ 8 个子命令 | 见下表 |
| `ciritty tpl <sub>` | `template` | ✅ 3 个子命令 | list/save/apply |

### 2. IPC msg 子命令

| 子命令 | 实现 | 备注 |
|---|---|---|
| `send-keys` | ✅ | `daemon/server/ipc.rs:14` |
| `list-panes` | ⚠️ | 实现完整但 **`cwd` 字段硬编码 `None`**（`ipc.rs:371`） |
| `info` | ✅ | session 详情 |
| `focus-pane` | ✅ | 含 agent diff 广播 |
| `close-pane` | ✅ | |
| `create-pane` | ✅ | |
| `run-command` | ⚠️ | 协议支持 `cwd`，**CLI 没暴露**（`main.rs:94` 硬编码 None） |
| `get-layout` | ✅ | 返回完整 LayoutState |

### 3. Keybind Actions（全部已实现）

| 类别 | Action | 默认绑定 | 文档化 |
|---|---|---|---|
| Navigation | FocusLeft/Right/Up/Down | Leader h/l/k/j | ✅ |
| Pane mgmt | CreatePane / SplitDown / ClosePane | Leader n / d / x | ✅ |
| Pane mgmt | MovePaneLeft/Right | Leader Shift+H/L | ✅ |
| Pane mgmt | ConsumeIntoColumn / ExpelFromColumn | Leader c / e | ✅ |
| Column width | ColumnWidthFull | Leader f | ✅ |
| Column width | ColumnWidthOneThird/Half/TwoThirds | （未默认绑定） | ❌ 隐藏 |
| Column width | Increase/Decrease/Cycle/Equalize | resize 模式 h/l/r/= | ✅ |
| Tile height | TileHeightIncrease/Decrease | （未默认绑定） | ❌ 隐藏 |
| Modes | EnterMode (scroll/move/resize) | Leader s/m/r | ✅ |
| Modes | ToggleBroadcast/Lock/Overview/Palette | Leader b / Ctrl+G / Tab / p | ✅ |
| Modes | ToggleSessionPalette | Leader Shift+p | ❌ 隐藏 |
| Sessions | NextSession/PrevSession/NewSession | Leader i / Shift+i / Shift+n | ❌ 隐藏 |
| Workspace | SwitchWorkspace(0..8) | Leader 1..9 | ❌ 隐藏 |
| Scroll mode | LineUp/Down PageUp/Down HalfPage Top/Bottom | j/k/u/d/f/b/g/G | ✅ |
| Direct | OpenSearch / Copy / Paste | Ctrl+Shift+F / C / V | ✅ |
| Other | Detach | Leader q | ✅ |
| Other | SendLeaderKey | 双击 Leader | ❌ 隐藏 |

---

## 终端协议 / 渲染功能

| 功能 | 状态 | 实现位置 |
|---|---|---|
| OSC 7 (cwd) | ✅ | `ciri-term/src/osc7_parser.rs` |
| OSC 8 (hyperlinks) | ✅ 4096 link map | `ciri-term/src/osc8_parser.rs` |
| OSC 9 (xterm notify) | ✅ | `ciri-term/src/pane/notify.rs:94` |
| OSC 133 (prompt marks A/B/C/D) | ✅ 解析齐全 | `ciri-term/src/shell_integration.rs` |
| OSC 777 (xterm notify ext) | ✅ | `ciri-term/src/pane/notify.rs:97` |
| Kitty graphics protocol | ✅ 376 行 | `ciri-term/src/kitty_graphics.rs` |
| Sixel | ✅ 613 行 | `ciri-term/src/sixel.rs` |
| Inline image rendering | ✅ | `ServerMessage::ImagePlacement` + `render.rs:2112` |
| Kitty keyboard protocol | ✅ | `ciri/src/app/key_encode/kitty.rs` |
| Legacy key encoding | ✅ | `ciri/src/app/key_encode/legacy.rs` |
| Ligatures（含 fi / Arabic 合字） | ✅ 带缓存 | `ciri-render/src/shaper.rs:76+` |
| 字体回退（cmap 解析） | ✅ | `ciri-render/src/font_resolver/` |
| IME / CJK | ✅ | `ciri/src/app/ime.rs` |
| 预测回显（mosh 风） | ✅ 三档：Never/Always/Adaptive | `ciri-app/src/prediction/` |
| 滚动条 | ✅ | `ciri-render/src/terminal/scrollbar.rs` |
| Bell（视觉+音频） | ✅ | `ciri/src/app/ui/bell_flash.rs`, `notification.rs` 内 audio |

---

## GPU 渲染 / 视觉

| 功能 | 状态 | 备注 |
|---|---|---|
| Blade 后端（Vulkan/Metal） | ✅ | `crates/ciri-gpu/src/blade.rs` |
| OpenGL 后端 | ✅ | `gl.rs` |
| DX11 后端 | ✅ | `dx.rs` |
| 分层渲染管线 | ✅ | `render-pipeline` 重构已合入 dev |
| Glyph atlas + alpha/color | ✅ | `ciri-render/src/glyph_cache/` |
| SDF 圆角 pane | ✅ | C1~C5 系列 |
| 焦点环 SDF | ✅ | C4 |
| 全局壁纸 / 背景图 | ✅ | `app/background_image.rs` |
| Pane 不透明度 / 失焦半透 | ✅ | `pane_opacity`, `inactive_opacity` |
| 动画（bezier / spring / easing） | ✅ 4 个 preset | `crates/ciri-anim/` |
| Glyph linear-correction | ✅ | render 中递归 |
| SDF 虚线边框 | ❌ | `render.rs:431,1575` 标记 TODO |
| 自定义 fragment shader | ❌ | 未暴露 hook |

---

## Multiplexer 核心

| 功能 | 状态 | 备注 |
|---|---|---|
| Session detach/attach | ✅ | 黄金标准 |
| 列布局 + workspace | ✅ | 独家形态 |
| 模板（template）保存/加载 | ✅ | `ciri-session/src/template.rs` |
| Session 持久化（agent-aware） | ✅ | claude/codex/opencode/droid 自动 resume |
| 多 client 共享 session（基础） | ✅ | `broadcast_to_session`，但 UI 没暴露"邀请/加入"流程 |
| 浮动 pane（zellij 招牌） | ❌ | |
| Stack 布局（zellij 招牌） | ❌ | |
| Workspace 跨多列 | ✅ | |
| 输入模式：prefix vs sticky | ✅ | `leader/handler.rs` |
| 广播输入到全部 pane | ✅ | `core.broadcast_mode` |
| Lock 模式（透传所有键） | ✅ | `Ctrl+G` |
| Overview 模式（缩略图） | ✅ | `Leader Tab` |
| 命令 palette | ✅ | `Leader p` |
| Session palette | ✅ | `Leader Shift+p` |
| 搜索 | ⚠️ 仅字面 | 无正则、无 case 切换 |
| 鼠标拖拽调列宽 | ✅ | `mouse.rs:600` |
| Focus follows mouse | ✅ | 配置项 |
| Copy-on-select | ✅ | 配置项 |
| 右键上下文菜单 | ✅ | Copy/Paste/SelectAll/Search |
| Tab 条（横向集成 + 纵向侧栏） | ✅ | `tab_bar/` 和 `top_bar/pane_tabs.rs` |
| 命名 paste buffer（tmux 风） | ❌ | |

---

## 网络 / 远程

| 功能 | 状态 |
|---|---|
| 二进制 RDP 风协议 | ✅ `ciri-protocol/` |
| 远程 attach 探测既有 sessions | ✅ `connection::probe_remote_sessions_blocking` |
| 远程 host 记忆（recent_hosts） | ✅ |
| SSH 隧道 | ✅ |
| Cell delta 增量编码 | ✅ |
| Full sync 编码 | ✅ |
| 协议 state machine | ✅ |
| 协议 benches | ✅ `protocol_decode.rs` |
| Web client / 浏览器 attach | ❌ |
| 多用户多光标协作（zellij 风） | ❌ | server 框架有，UI 没暴露 |
| Mosh 风 UDP / QUIC | ❌ | 现在走 TCP |

---

## 插件 / 扩展

| 功能 | 状态 | 备注 |
|---|---|---|
| Lua VM | ✅ | `ciri-plugin/src/vm.rs` |
| 沙箱 | ✅ | `sandbox.rs` |
| 事件系统（含失败计数） | ✅ | `events.rs`，3 次失败自动禁用 |
| 内建插件：session_restore | ✅ | agent 检测 |
| 内建插件：usage（Claude/Codex） | ✅ | `builtin/usage.lua` |
| Lua API 文档 | ⚠️ 内联注释 | 无独立 docs |
| 插件 install / 包管理 | ❌ | |
| 插件 marketplace / registry | ❌ | |

---

## Shell 集成

| 功能 | 状态 | 备注 |
|---|---|---|
| bash 集成脚本 | ✅ | `shell-integration/ciri.bash` |
| zsh 集成脚本 | ✅ | `ciri.zsh` |
| fish 集成脚本 | ✅ | `ciri.fish` |
| OSC 133 解析 | ✅ | A/B/C/D 完整 |
| Prompt 跳转（Cmd+Up/Down） | ❌ | 数据有，UI 没做 |
| 命令耗时显示 | ❌ | |
| Exit code 染色 | ❌ | 通知用了 exit code，渲染没用 |
| 滚动条按 prompt 染色 | ❌ | |

---

## 平台支持

| 功能 | macOS | Linux | Windows |
|---|---|---|---|
| GUI 启动 | ✅ | ✅ | ✅ |
| GPU 后端（自动选择） | Blade(Metal) | Blade(Vulkan) / GL | DX11 |
| PTY | ✅ portable-pty | ✅ portable-pty | ✅ ConPTY |
| 系统托盘 | ✅ | ✅ | ✅ |
| 自启动 | ✅ | ✅ | ✅ |
| 桌面通知 | ✅ notify_rust | ✅ notify_rust | ❌ stub |
| Procinfo（前台进程） | ✅ proc_pidinfo | ✅ /proc/PID | ⚠️ 半成品 TODO |
| Agent 检测（procinfo 依赖） | ✅ | ✅ | ❌ 受 procinfo 影响 |

---

## 用量统计 / AI 集成

| 功能 | 状态 |
|---|---|
| Claude OAuth 探测 | ✅ `crates/ciri/src/app/usage/claude.rs` |
| Codex OAuth 探测 | ✅ `usage/codex.rs` |
| Token 管理 | ✅ `tokens.rs` |
| 周期轮询 | ✅ `poller.rs` |
| 快照 | ✅ `snapshot.rs` |
| Lua 自定义 format（按 pane 切换） | ✅ `builtin/usage.lua` |
| 状态栏展示 | ✅ |
| AI inline 解释（选中 → 解释） | ❌ |
| Scrollback 摘要 | ❌ |
| Agent attach pane（专门 pane） | ❌ |
| 命令 ghost-text 建议 | ❌ |

---

## 配置 / 主题

| 功能 | 状态 | 备注 |
|---|---|---|
| TOML 配置 | ✅ | `~/.config/ciri/config.toml` |
| 配置热重载 | **✅** | `event.rs:443` watcher → `reload_config()` |
| 配置校验 | ✅ | bad value 警告 |
| Init wizard | ✅ | `init::run_init()` |
| 主题预设 | ✅ 8 个 | ciri_dark / one_dark / catppuccin_mocha / tokyo_night / dracula / nord / gruvbox_dark / ghostty |
| 主题自定义（TOML 覆盖） | ✅ | |
| 导入 Alacritty 配色 | ❌ | |
| 导入 iTerm2 .itermcolors | ❌ | |
| 导入 tmux.conf | ❌ | |
| 导入 zellij.kdl | ❌ | |
| 多 profile（多 shell 配置） | ❌ | |
| 项目级 auto-layout（按 cwd） | ❌ | 模板有，cwd 触发器无 |

---

## 半成品 / 已知缺陷

| 项 | 文件 | 影响 |
|---|---|---|
| `list-panes` 的 `cwd` 永远是 `None` | `daemon/server/ipc.rs:371` | JSON/CLI 输出 cwd 始终为空 |
| `run-command` 不能传 `--cwd` | `main.rs:94` | 协议有 cwd 字段，CLI 没暴露 |
| Windows procinfo 半成品 | `ciri-procinfo/src/windows.rs:8,176,185` | Win 上 agent 自动恢复 / cwd 探测失效 |
| Windows 桌面通知无实现 | `notification.rs:38` | Win 上长命令完成不通知 |
| SDF 虚线边框 TODO | `render.rs:431,1575` | 视觉细节 |
| 搜索无正则 / case 切换 | `app/keyboard.rs` 周边 | 仅字面匹配 |
| 多 client UI 未暴露 | server 层已就绪 | 没有"分享 session"用户流程 |

---

## 完全没做（按战略价值排）

### 🔴 紧迫（AI / 多路复用核心方向）

| 项 | 价值 |
|---|---|
| `msg capture-pane` | AI 工作流刚需：pane 输出喂 LLM 的基石 |
| `msg pipe-pane` | 流式输出 tee（持续日志 / AI 实时分析） |
| AI inline 解释（选中 → 流式答复到下个 pane） | 独家差异化 |
| Agent attach pane（pane 类型） | 把 Claude Code / Codex 一等公民化 |
| 浮动 pane | 抢 zellij 招牌；lazygit/gh dash 类高频 |
| Stack 布局 | "列 + stack" 是独家组合形态 |

### 🟡 中等（用户体验 / 生态）

| 项 | 价值 |
|---|---|
| Shell 集成 UX（Cmd+Up/Down 跳 prompt、exit code 染色、耗时显示） | OSC 133 已解析，差渲染落地 |
| Hints / URL 键盘 picker | kitty 招牌，对你 leader 体系是天然延伸 |
| 命名 paste buffer | tmux 用户日用 |
| `rename-session` / `msg rename` | tmux 等价 |
| `reload-config` 信号入口（除了文件 watcher） | 远程 server 端配置怎么走没验证 |
| 桌面通知 Windows 实现 | 用 winrt-notification 或 PowerShell |
| 协议有 / CLI 缺：`switch-workspace` / `switch-session` / `resize-pane` / `set-col-width` | 一个下午能补齐 |

### 🟢 长期（差异化 / 出圈）

| 项 | 价值 |
|---|---|
| 多 cursor 协作（zellij 招牌） | 配对编程 |
| Web client（zellij 新） | 现成二进制协议 + WS bridge |
| 配置导入器（tmux/zellij/iterm/alacritty） | 拉存量用户最便宜 |
| 插件 marketplace + `ciritty plugin install` | 从工具变平台 |
| 自定义 fragment shader hook | Ghostty 出圈点 |
| Quake / 下拉模式 + 全局热键 | Ghostty / WT 都有 |
| Mosh 风 UDP / QUIC 传输层 | 网络抖动 / roaming |
| 多 profile（多 shell 一窗） | Win 用户首要诉求 |
| 搜索正则 / case toggle | UX 改进 |

### 🔵 不必追

| 项 | 原因 |
|---|---|
| ssh-kitten 等价（terminfo copy） | 你有自己的二进制协议，不必走 SSH 套壳 |
| GUI 配置器 | 目标用户更愿编辑 toml |
| 跑在任意 host 终端内 | 结构性差异，不要倒退 |

---

## 与主流终端横向对比（核对后版本）

| 维度 | tmux | zellij | Ghostty | Kitty | WT | **ciritty** |
|---|---|---|---|---|---|---|
| 内建 multiplexer | ✅ | ✅ | ❌ | △ | ❌ | ✅ 列+workspace |
| 网络远程 attach | ❌ | ❌ | ❌ | △ | ❌ | ✅ 二进制 |
| 自有 GPU 渲染 | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ 三后端 |
| Win 原生 | ❌ | ❌ | ⏳ | ❌ | ✅ | ✅ |
| 预测回显 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| 配置热重载 | ✅ | ✅ | ✅ | ✅ | ✅ | **✅**（之前错判） |
| 桌面通知 OSC 9 | ❌ | △ | ✅ | ✅ | △ | **✅ Unix**，Win stub |
| Kitty graphics | ❌ | △ | ✅ | ✅ | ❌ | ✅ |
| Sixel | △ | △ | ✅ | ✅ | △ | ✅ |
| 浮动 pane | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Stack 布局 | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Hints/URL picker | ❌ | △ | ✅ | ✅ | ❌ | ❌ |
| Quake 模式 | ❌ | ❌ | ✅ | ✅ | ✅ | ❌ |
| 多 profile | ❌ | △ | △ | △ | ✅ | ❌ |
| 多 cursor 协作 | △ | ✅ | ❌ | ❌ | ❌ | △ 框架有，UI 无 |
| Web client | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| 插件 marketplace | ✅ TPM | △ | ❌ | ✅ | ❌ | ❌ |
| Sticky 输入模式 | ❌ | ✅ | ❌ | ❌ | ❌ | **✅**（之前漏写） |
| Tab 条 | ✅ | ✅ | ✅ | ✅ | ✅ | **✅**（横向+纵向） |
| Agent-aware session restore | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ 独家 |
| 内联图像 | ❌ | △ | ✅ | ✅ | ❌ | ✅ |
| Ligature | △ | △ | ✅ | ✅ | ✅ | ✅ |
| 主题预设数 | 用户社区 | 用户社区 | 250+ | 用户社区 | 用户社区 | 8 内置 |

---

## TL;DR

ciritty **比我前几轮断言的要完整得多**。被我误判"缺失"的功能里：
- 配置热重载 / 桌面通知 / Tab 条 / Sticky 模式 / 多 client 框架 都已存在
- OSC 7/8/9/133/777 解析齐全
- 三个 GPU 后端、列布局、Lua 沙箱、预测回显、agent-aware restore 是真正的独家武器

真正的短板是：
1. **AI 工作流的脚本基石**（capture-pane / pipe-pane / agent pane）
2. **Shell 集成数据没落地到 UI**（OSC 133 parser 有，jump-to-prompt UI 没做）
3. **Windows 半成品**（procinfo + 通知）
4. **zellij 招牌没拿**（浮动 pane / stack / 多 cursor 协作）
5. **配置生态封闭**（无导入、无插件包管理）

建议下一步先攻：`msg capture-pane` + Shell 集成 UX 落地 + Windows procinfo —— 这三个都能直接解锁现有差异化的真正威力（AI 工作流、prompt 跳转、Win 上 agent 恢复）。
