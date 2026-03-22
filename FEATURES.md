# Ciri — 功能与特性全览

> GPU 加速终端复用器，将无限滚动平铺窗口管理引入终端。

---

## 目录

- [核心理念](#核心理念)
- [布局系统](#布局系统)
- [键绑定](#键绑定)
- [CLI 命令](#cli-命令)
- [终端仿真](#终端仿真)
- [渲染引擎](#渲染引擎)
- [会话管理](#会话管理)
- [布局模板](#布局模板)
- [IPC 脚本接口](#ipc-脚本接口)
- [远程会话](#远程会话)
- [配置系统](#配置系统)
- [主题系统](#主题系统)
- [交互特性](#交互特性)
- [协议与架构](#协议与架构)
- [平台支持](#平台支持)

---

## 核心理念

Ciri 是一个终端复用器，核心差异化在于 **无限滚动平铺布局**（灵感来自 Niri compositor）+ **GPU 加速渲染**：

- 面板按列排列，列在水平轴上无限延伸
- 每列内可垂直堆叠多个面板（Tile）
- 聚焦列自动居中，左右列可见但可裁剪
- 弹簧动画驱动所有过渡效果
- Client-Server 架构，支持会话持久化和多客户端

```
┌──────────────────────────────────────────────────────────────────┐
│ [workspace: backend]                                             │
│                                                                  │
│  ◄ scroll ──────────────────────────────────────────── scroll ►  │
│                                                                  │
│  ┌─────────┐  ┌─────────┐  ┌──────────────┐  ┌─────────┐       │
│  │ nvim    │  │ cargo   │  │   lazygit    │  │ htop    │       │
│  │         │  │ watch   │  │              │  │         │       │
│  │         │  │         │  │              │  │         │       │
│  │         │  ├─────────┤  │              │  │         │       │
│  │         │  │ tail -f │  │              │  │         │       │
│  │         │  │ logs    │  │              │  │         │       │
│  └─────────┘  └─────────┘  └──────────────┘  └─────────┘       │
│       60%         40%            80%              40%            │
│  [col 1]      [col 2]       [col 3/focus]    [col 4]            │
└──────────────────────────────────────────────────────────────────┘
```

---

## 布局系统

### 2D 布局模型

| 层级 | 说明 |
|------|------|
| **WorkspaceSet** | 所有 workspace 的容器，支持垂直切换 |
| **Workspace** | 一行水平排列的列，无限向右扩展 |
| **Column** | 一列中垂直堆叠的 tile，可设置宽度 |
| **Tile** | 单个终端面板，绑定到一个 PTY |

### 列宽控制

- **比例宽度**：相对于视口的百分比（如 0.5 = 50%）
- **固定像素**：以像素为单位的绝对宽度
- **预设循环**：默认 1/3 → 1/2 → 2/3 → 全宽，可自定义
- **交互式调整**：拖拽列边框、快捷键增减宽度
- **相邻列均分**：一键让两列等宽

### Tile 高度控制

- **权重分配**：列内 tile 按 weight 比例分配高度
- **拖拽调整**：鼠标拖拽 tile 之间的水平边框
- **Consume/Expel**：将相邻列的面板吸入当前列 / 将当前 tile 弹出为新列

### 居中策略

| 策略 | 行为 |
|------|------|
| `always` | 聚焦列始终居中显示 |
| `on-overflow` | 仅当列宽超出视口时居中 |
| `never` | 左对齐滚动 |

---

## 键绑定

Leader key 默认为 `Ctrl+W`，可在配置中修改。支持 `prefix`（tmux 风格，按一次 leader 执行一个动作）和 `sticky`（zellij 风格，保持 leader 模式直到 Esc）两种模式。

### 面板生命周期

| 绑定 | 动作 |
|------|------|
| `Leader + n` | 在右侧创建新列 |
| `Leader + d` | 在下方创建新 workspace |
| `Leader + x` | 关闭当前面板 |

### 导航

| 绑定 | 动作 |
|------|------|
| `Leader + h` / `←` | 聚焦左列 |
| `Leader + l` / `→` | 聚焦右列 |
| `Leader + j` / `↓` | 聚焦下方（列内 tile 或下一个 workspace） |
| `Leader + k` / `↑` | 聚焦上方（列内 tile 或上一个 workspace） |

### 面板移动

| 绑定 | 动作 |
|------|------|
| `Leader + Shift+h` | 将当前列左移 |
| `Leader + Shift+l` | 将当前列右移 |

### 列宽

| 绑定 | 动作 |
|------|------|
| `Leader + r` | 循环预设宽度（正向） |
| `Leader + Shift+r` | 循环预设宽度（反向） |
| `Leader + f` | 全宽 |
| `Leader + [` | 减小列宽 |
| `Leader + ]` | 增大列宽 |
| `Leader + =` | 与右邻列均分 |

### Tile 管理

| 绑定 | 动作 |
|------|------|
| `Leader + c` | 将右列的活动面板吸入当前列 |
| `Leader + v` | 将当前 tile 弹出为新列 |

### 模式切换

| 绑定 | 动作 |
|------|------|
| `Leader + o` / `Tab` | 切换 Overview 模式 |
| `Leader + p` | 切换 Command Palette |
| `Leader + b` | 切换广播模式（输入发送到所有可见面板） |
| `Leader + q` | Detach（断开客户端，服务端继续运行） |

### Workspace 切换

| 绑定 | 动作 |
|------|------|
| `Leader + 1-9` | 切换到 workspace 1-9 |

### 滚动

| 绑定 | 动作 |
|------|------|
| `Ctrl+Shift+F` | 搜索 scrollback |
| 鼠标滚轮 | 滚动 scrollback |
| `PageUp` / `PageDown` | 翻页 |

### Leader Key 双击

快速双击 leader key 将把 leader 字符本身发送到终端。

---

## CLI 命令

### 会话管理

```bash
ciri                              # 创建新会话并连接
ciri new                          # 同上
ciri <name>                       # 连接到会话（不存在则创建）
ciri attach|a <name>              # 附加到已有会话（不存在则报错）
ciri list|ls                      # 列出所有会话（运行中 + 已保存）
ciri kill|k <name>                # 终止会话
ciri kill-server|ks               # 终止整个服务进程
ciri delete|rm <name>             # 删除已保存的会话文件
```

### 远程连接

```bash
ciri remote <user@host> [session]     # 通过 SSH 隧道连接远程服务器
    --port <port>                     # 远程 ciri TCP 端口（默认 7890）
    --ssh-port <port>                 # SSH 端口（默认 22）
```

### IPC 脚本接口

```bash
ciri msg send-keys <session> <pane_id> <keys>     # 发送按键到面板
ciri msg list-panes <session> [--json]             # 列出会话中的面板
ciri msg info <session> [--json]                   # 获取会话详情
ciri msg focus-pane <session> <pane_id>            # 聚焦指定面板
ciri msg close-pane <session> <pane_id>            # 关闭指定面板
ciri msg create-pane <session> [--json]            # 创建新面板
ciri msg get-layout <session> [--json]             # 获取完整布局状态
ciri msg run-command <session> <command>            # 在新面板运行命令
```

### 布局模板

```bash
ciri template list                                 # 列出所有模板
ciri template apply <name> [session]               # 用模板创建/重建会话
ciri template save <name> <session>                # 将当前布局导出为模板
ciri tpl list                                      # 别名
```

---

## 终端仿真

基于 `alacritty_terminal` 的完整 VT100/xterm 终端仿真器。

### 基础能力

| 特性 | 说明 |
|------|------|
| 真彩色 | 16 色 / 256 色 / RGB 全支持 |
| 文本样式 | 粗体、斜体、暗淡、下划线（单/双/波浪/点状/虚线）、删除线、反色、隐藏 |
| 光标样式 | Block、Underline、Beam、Hidden、HollowBlock |
| 光标闪烁 | 可配置间隔 |
| CJK 字符 | 正确的双宽字符渲染 |
| Unicode | 完整的 grapheme cluster 支持（emoji、ZWJ 序列、组合标记） |
| 交替屏幕 | Alt screen buffer（TUI 应用支持） |
| Scrollback | 可配置的回滚历史行数（默认 10,000） |

### 高级协议

| 协议 | 说明 |
|------|------|
| **Kitty 键盘协议** | CSI u 编码，精确按键上报 |
| **SGR 鼠标** | 按钮、位置、修饰键全支持 |
| **Bracketed Paste** | DECSET 2004，粘贴内容自动包裹 |
| **Focus Events** | DECSET 1004，窗口聚焦/失焦通知 |
| **Synchronized Output** | DEC 2026，缓冲更新减少闪烁 |
| **Shell Integration** | OSC 133 提示符标记（prompt/input/output 区域） |
| **OSC 8 超链接** | 内联超链接，鼠标悬停检测 |
| **OSC 52 剪贴板** | 终端应用直接写入系统剪贴板 |
| **Kitty 图片协议** | 多块累积传输，占位符渲染 |
| **Sixel 图片** | 完整 Sixel 解码，RGBA 像素转换 |

---

## 渲染引擎

### GPU 后端

| 后端 | API | 平台 |
|------|-----|------|
| **Blade**（默认） | Vulkan / Metal | Linux, macOS |
| **GL** | OpenGL 3.3+ / EGL | Linux (Wayland native) |
| **DX** | Direct3D 11 | Windows |
| **Auto** | 自动选择最佳后端 | 全平台 |

### 渲染特性

- **字形 Atlas**：shelf-based bin packing，支持 RGBA 彩色 emoji
- **文本整形**：rustybuzz (HarfBuzz) 连字检测 + grapheme shaping
- **字体回退**：fontconfig (Unix) / fontdb (Windows)，emoji 字体自动回退
- **增量渲染**：逐行脏标记（per-row dirty flags）
- **视口裁剪**：仅渲染可见面板
- **零拷贝热路径**：`PackedCell`（14 字节 POD）通过 `bytemuck::cast_slice` 直接传输

### 视觉效果

- **焦点环**：Solid / Glow（多层阴影） / Dashed 三种样式
- **面板动画**：Fade / SlideUp / SlideDown / SlideLeft / FadeSlideUp
- **焦点过渡**：弹簧动画驱动的不活跃面板透明度
- **Bell 通知**：150ms 淡出视觉铃声
- **IME 预编辑**：候选文字覆盖层
- **滚动条**：轨道 + 拇指，可拖拽

---

## 会话管理

### 持久化

- 会话状态保存为 JSON：`$XDG_STATE_HOME/ciri/sessions/<name>.json`
- 原子写入：写入临时文件 → rename
- 自动保存：布局变更后 250ms 去抖
- 保存内容：workspace 结构、列宽、tile 权重、active 索引
- 附加时恢复 scrollback（最多 1000 行）

### 多客户端

- 同一会话支持多个客户端同时连接
- 最小视口策略：所有客户端中最小的视口尺寸生效
- 每客户端独立的 viewport offset 和动画状态
- 客户端断开时会话继续运行

### 会话名称

- 自动生成（基于形容词+名词组合）
- 用户自定义
- 名称校验：防止路径遍历攻击

---

## 布局模板

TOML 格式的布局模板，存储在 `~/.config/ciri/templates/` 目录。

### 模板格式

```toml
description = "开发工作区：编辑器 + 双终端"

[[workspaces]]
active_column = 0

[[workspaces.columns]]
width = { proportion = 0.6 }

[[workspaces.columns.tiles]]
command = "nvim"
cwd = "/home/user/projects"

[[workspaces.columns]]
width = { proportion = 0.4 }

[[workspaces.columns.tiles]]
command = ""          # 空 = 默认 shell
cwd = ""              # 空 = 继承
weight = 1.0

[[workspaces.columns.tiles]]
command = ""
weight = 1.0
```

### 模板字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `description` | string? | 可选描述 |
| `workspaces[].active_column` | usize | 初始聚焦列（默认 0） |
| `workspaces[].columns[].width` | `{proportion}` 或 `{fixed}` | 列宽 |
| `workspaces[].columns[].tiles[].command` | string | 启动命令（空 = shell） |
| `workspaces[].columns[].tiles[].cwd` | string | 工作目录（空 = 继承） |
| `workspaces[].columns[].tiles[].weight` | f64 | 高度权重（默认 1.0） |

---

## IPC 脚本接口

通过 Unix socket 暴露命令接口，复用服务端已有的 `__control__` 会话模式。所有命令支持 `--json` 标志输出 JSON 格式。

### JSON 输出示例

```bash
$ ciri msg list-panes dev --json
[
  {
    "pane_id": 1,
    "cols": 120,
    "rows": 40,
    "title": "nvim",
    "cwd": null,
    "is_active": true,
    "workspace_idx": 0,
    "column_idx": 0,
    "tile_idx": 0
  },
  ...
]
```

```bash
$ ciri msg info dev --json
{
  "name": "dev",
  "running": true,
  "pane_count": 3,
  "client_count": 1,
  "workspace_count": 1,
  "active_workspace": 0
}
```

---

## 远程会话

本地 Ciri 客户端通过 SSH 隧道连接远程 Ciri 服务器。

### 工作原理

```
[本地 ciri 客户端] ──stdin/stdout──▶ [ssh -W localhost:7890 user@host] ──TCP──▶ [远程 ciri-server :7890]
```

1. 远程服务器配置 `[remote] enabled = true`，监听 `127.0.0.1:7890`
2. 客户端执行 `ciri remote user@host`
3. 客户端 fork `ssh -W` 进程建立 stdio 代理隧道
4. 相同的握手 + 帧协议通过隧道传输

### 安全性

- TCP 监听 **仅绑定 `127.0.0.1`**，不暴露到网络
- `remote.enabled` 默认 `false`，需显式开启
- 所有认证和加密由 SSH 处理
- 隧道断开时客户端显示 Disconnected

---

## 配置系统

配置文件路径：`$XDG_CONFIG_HOME/ciri/config.toml`（默认 `~/.config/ciri/config.toml`）

### 完整配置参考

```toml
[font]
family = "monospace"                  # 字体族
size = 14.0                           # 字号

[appearance]
padding = 4.0                         # 面板内边距 (px)
column_gap = 8.0                      # 列间距 (px)
border_width = 2.0                    # 边框宽度 (px)
active_border_color = ""              # 活跃面板边框色 (hex)
inactive_border_color = ""            # 非活跃面板边框色 (hex)
inactive_opacity = 0.7                # 非活跃面板不透明度

[appearance.focus_ring]
style = "solid"                       # solid / glow / dashed
glow_radius = 4.0                     # Glow 模式半径
glow_layers = 3                       # Glow 层数
dash_length = 8.0                     # Dashed 模式线段长
gap_length = 4.0                      # Dashed 模式间隙长

[animation]
enabled = true                        # 是否启用动画
speed = 12.0                          # 弹簧动画速度
epsilon = 0.1                         # 收敛阈值
pane_open_style = "fade"              # fade / slide-up / slide-down / slide-left / fade-slide-up
pane_open_duration_ms = 200           # 面板打开动画时长
pane_close_duration_ms = 150          # 面板关闭动画时长
focus_transition_speed = 15.0         # 焦点过渡速度
overview_zoom_fit = 0.9               # Overview 缩放适配系数
zoom_threshold = 0.99                 # 缩放动画阈值

[window]
width = 1024.0                        # 初始窗口宽度
height = 768.0                        # 初始窗口高度
title = "ciri"                        # 窗口标题

[terminal]
shell = ""                            # Shell 路径（空 = 平台默认）
default_cols = 80                     # 默认列数
default_rows = 24                     # 默认行数
scrollback_lines = 10000              # 回滚行数（0 = 禁用）
cursor_color = "#E6E6E6"              # 光标颜色
cursor_opacity = 0.7                  # 光标不透明度
cursor_blink = true                   # 光标闪烁
cursor_blink_interval_ms = 500        # 闪烁间隔

[statusbar]
padding_ratio = 0.25                  # 垂直内边距比例
text_baseline = 0.8                   # 文字基线因子
leader_indicator_ratio = 0.1          # Leader 指示器高度比例

[input]
leader_timeout_ms = 1000              # Leader 超时时间
double_tap_window_ms = 300            # 双击窗口
scroll_multiplier = 50.0              # 滚动倍率
mode = "prefix"                       # prefix (tmux) / sticky (zellij)

[render]
frame_interval_ms = 16                # 帧间隔（16 ≈ 60fps）
atlas_size = 2048                     # 字形 Atlas 纹理尺寸
max_glyph_instances = 32768           # 每帧最大字形数
max_rectangles = 8192                 # 每帧最大矩形数
frame_latency = 2                     # 帧延迟
present_mode = "fifo"                 # fifo (vsync) / mailbox / immediate
backend = "auto"                      # auto / blade / gl / dx

[layout]
center_focused_column = "always"      # always / on-overflow / never
# default_column_width = { proportion = 0.5 }
# preset_widths = [
#   { proportion = 0.333 },
#   { proportion = 0.5 },
#   { proportion = 0.667 },
#   { proportion = 1.0 },
# ]

[gesture]
enabled = true                        # 触控板手势
pinch_sensitivity = 2.0               # 捏合灵敏度
natural_scroll = true                 # 自然滚动方向
vertical_swipe_threshold = 50.0       # 垂直滑动阈值 (px)
horizontal_swipe_threshold = 50.0     # 水平滑动阈值 (px)
smooth_scroll = true                  # 平滑滚动
scroll_pixels_per_line = 20.0         # 每行对应像素数

[remote]
enabled = false                       # TCP 监听开关
port = 7890                           # TCP 端口（仅 127.0.0.1）

[keys]
leader = "ctrl+w"                     # Leader key

[keys.bindings]
# n = "new_column_right"
# x = "close_pane"
# 自定义绑定...

[keys.overview_bindings]
# Overview 模式绑定...
```

### 热重载

配置文件修改后自动检测并应用以下变更（无需重启）：
- 字体变更
- Leader key 变更
- 键绑定变更
- 主题变更

### 配置校验

加载时自动校验并修复不合理值：
- `animation.speed <= 0` → 重置为 12.0
- `font.size <= 0` → 重置为 14.0
- `inactive_opacity` 超出 [0, 1] → clamp
- `frame_interval_ms == 0` → 重置为 16
- `$HOME` 未设置时使用 `getpwuid_r` 回退

---

## 主题系统

### 内置主题

| 主题名 | 风格 |
|--------|------|
| `one_dark` | Atom One Dark（默认） |
| `catppuccin_mocha` | Catppuccin Mocha |
| `tokyo_night` | Tokyo Night |
| `dracula` | Dracula |
| `nord` | Nord |
| `gruvbox_dark` | Gruvbox Dark |

### 颜色字段

```toml
[theme]
preset = "one_dark"           # 使用预设

# 覆盖单个颜色
foreground = "#ABB2BF"
background = "#282C34"
black = "#282C34"
red = "#E06C75"
green = "#98C379"
yellow = "#E5C07B"
blue = "#61AFEF"
magenta = "#C678DD"
cyan = "#56B6C2"
white = "#ABB2BF"
bright_black = "#5C6370"
bright_red = "#E06C75"
# ... bright_* 系列

# UI 专用颜色
ui_background = "#21252B"
overview_background = "#1E2227"
statusbar_background = "#21252B"
border_active = "#61AFEF"
border_inactive = "#3E4452"
accent = "#61AFEF"
statusbar_dim = "#5C6370"
mode_broadcast = "#E5C07B"
```

主题支持 **字段级覆盖合并**：先加载预设，再用用户配置中的颜色字段覆盖。

---

## 交互特性

### Overview 模式

全局鸟瞰视图，展示所有 workspace 和面板：
- 面板以缩略图形式渲染
- 支持 hjkl 导航
- 支持创建/关闭面板
- 捏合缩放
- 拖拽平移
- Enter/Esc 退出

### Command Palette

模糊搜索命令面板：
- 按标题/CWD/命令搜索面板
- 切换会话
- 执行 Action

### 广播模式

`Leader + b` 开启后，键盘输入同时发送到当前 workspace 的所有可见面板。状态栏显示广播指示器。

### 鼠标交互

| 操作 | 行为 |
|------|------|
| 左键单击 | 聚焦面板 / 开始选择 |
| 左键双击 | 选中单词 |
| 左键三击 | 选中整行 |
| 滚轮 | 滚动 scrollback |
| 拖拽列边框 | 调整列宽 |
| 拖拽 tile 边框 | 调整 tile 高度 |
| 拖拽滚动条 | 精确滚动 |
| 悬停链接 | 显示下划线 |

### 触控板手势

| 手势 | 行为 |
|------|------|
| 双指水平滑动 | 切换列 |
| 双指垂直滑动 | 切换 workspace |
| 捏合 | Overview 缩放 |
| Shift+滚动 | workspace 切换 |

### 剪贴板

- `Ctrl+Shift+C` / 选中后右键 → 复制
- `Ctrl+Shift+V` → 粘贴
- OSC 52 → TUI 应用直接写入剪贴板

### 搜索

- `Ctrl+Shift+F` → 打开搜索
- 高亮所有匹配项
- `Enter` / `Shift+Enter` → 在匹配项间跳转

### 内联图片

- Kitty 图片协议（多块传输，占位符渲染）
- Sixel 图片（完整解码为 RGBA 像素）
- 每面板最多保留 64 个活跃图片

### IME 输入法

- 预编辑文字覆盖层
- 光标位置跟踪
- 下划线指示预编辑范围

---

## 协议与架构

### Client-Server 通信

```
┌─────────────────────────────────┐
│  ciri (Client)                  │
│  - winit event loop             │
│  - GPU rendering (wgpu)         │
│  - Layout animation             │
│  - User input handling          │
└──────────────┬──────────────────┘
               │ Unix socket / TCP / Named pipe
┌──────────────▼──────────────────┐
│  ciri-server (Daemon)           │
│  - Accept client connections    │
│  - Manage panes (PTY)           │
│  - Handle layout operations     │
│  - Tick-based update loop 16ms  │
│  - Session persistence          │
└─────────────────────────────────┘
```

### 帧格式

```
[tag: u8][len: u32 LE][payload: N bytes]
```

| Tag | 用途 |
|-----|------|
| `0x01` | ClientMessage (msgpack) |
| `0x10` | ServerMessage (msgpack) |
| `0x20` | CellDelta (自定义二进制，增量更新) |
| `0x21` | FullPaneSync (自定义二进制，全量快照) |

### PackedCell 格式（14 字节零拷贝）

```
[char: 4B UTF-8][fg: 4B PackedColor][bg: 4B PackedColor][flags: 2B LE]
```

### 握手流程

1. Client 发送 `ClientHello`：magic("CIRI") + 版本 + wire 版本 + 会话名 + 视口尺寸
2. Server 回复 `ServerHello`：magic + 版本
3. Server 发送 `StateSync` + 每个面板的 `FullPaneSync`
4. 进入正常帧循环

### 流控

- 基于 generation/ack 的闭环
- 服务端每 16ms tick 一次，累积 damage
- 客户端确认收到帧后服务端才发送下一帧
- DEC 2026 同步输出模式：延迟发送直到同步结束

---

## 平台支持

| 平台 | 传输 | GPU 后端 | 状态 |
|------|------|----------|------|
| **Linux** | Unix socket | Blade (Vulkan) / GL (EGL) | 主要平台 |
| **macOS** | Unix socket | Blade (Metal) | 支持 |
| **Windows** | Named pipe | DX (Direct3D 11) | 支持 |

---

## 代码规模

**26,604 行 Rust 代码**，11 个 crate：

| Crate | 行数 | 职责 |
|-------|-----:|------|
| ciri | 8,093 | 客户端（App / 渲染 / 输入 / CLI） |
| ciri-gpu | 4,303 | GPU 后端（Blade / GL / DX） |
| ciri-render | 2,941 | 终端视图构建 + 字形缓存 |
| ciri-server | 2,642 | 服务端守护进程 |
| ciri-protocol | 2,404 | 协议编解码 + 消息定义 |
| ciri-term | 2,360 | PTY + 终端仿真 |
| ciri-layout | 1,420 | 2D 布局引擎 |
| ciri-config | 888 | TOML 配置系统 |
| ciri-input | 822 | Leader key + 键绑定 |
| ciri-session | 414 | 会话持久化 + 模板 |
| ciri-anim | 317 | 弹簧动画 |
