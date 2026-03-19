# Ciri 产品定位与功能规划

## 一句话定位

**Ciri 是一个 GPU 加速的滚动式终端复用器，用无限滚动列取代传统的固定分屏，让终端窗口管理像浏览网页一样自然。**

---

## 竞品格局

| 产品 | 类型 | 核心卖点 | 致命短板 |
|------|------|----------|----------|
| tmux | 纯复用器 | 无处不在，远程 session 保活 | 配置地狱，UI 原始，学习曲线陡峭 |
| Zellij | 纯复用器 | WASM 插件，浮动窗格，swap layout | 吞吐性能不如 tmux，依赖宿主终端渲染 |
| Ghostty | 纯终端 | 平台原生 UI，极致性能 | 无内置复用，无远程 session |
| Alacritty | 纯终端 | 极简哲学，Vi mode | 不做分屏，不做复用，刻意不做 |
| WezTerm | 终端+复用 | Lua 配置万能，SSH domain | 开发停滞，性能一般，非原生 UI |

**空白地带：没有人做"GPU 渲染 + 滚动式 layout + session 保活"的一体化方案。**

- tmux/Zellij 受限于宿主终端的渲染能力，动画和视觉表现力有天花板
- Ghostty/Alacritty 不做复用，你还是得套 tmux
- WezTerm 做了复用但 layout 模型是传统 tab+split，且项目风险大

Ciri 的机会：**自己渲染 + 自己复用 + 独特的滚动式 layout**，三合一。

---

## Ciri 的差异化核心

### 1. 滚动式 layout（vs 传统固定分屏）

这是 ciri 最根本的差异点。tmux/Zellij/WezTerm 都是"在固定屏幕内切分"——你开 4 个 pane，每个只能占 1/4 屏幕，挤得不可用。

Ciri 的做法：pane 排成无限长的横向列，viewport 只显示其中一段。每个 pane 可以是 viewport 宽度的 1/3 到 100%。看不见的 pane 不消失，只是滚出了视口。

**这意味着：**
- 你可以同时开 10 个 pane 而不用担心每个都太小
- 常用的 pane 设为大宽度，不常用的设为窄宽度
- 切换 pane 是平滑滚动，不是瞬间跳转
- 两个相邻 pane 可以同时在视口内可见

**类比：** tmux 是在一张纸上画格子，Ciri 是一个可以左右滚动的画布。

### 2. GPU 直接渲染（vs 依赖宿主终端）

tmux 和 Zellij 的视觉效果受限于运行它们的终端模拟器。Ciri 自己就是终端，用 wgpu 直接做 GPU 渲染：
- 丝滑的弹簧物理动画（滚动、resize、overview 缩放）
- 未来可以做 Zellij 做不到的事：半透明 pane、模糊背景、自定义着色器

### 3. Client-Server 分离（vs 单进程）

与 Ghostty/Alacritty 不同，Ciri 是 client-server 架构：
- 关掉窗口，session 不死（像 tmux）
- 多个 client 可以 attach 同一个 session（像 tmux 的 pair programming）
- 未来可以做远程 attach（SSH tunnel，替代 tmux 的核心场景）

---

## 功能规划

### 第一阶段：把 layout 做到极致

这是 ciri 的灵魂，必须先做好。

#### 1.1 可配置的宽度预设循环

**现状：** 硬编码 1/3、1/2、2/3、full 四个档位，快捷键混乱。
**目标：** 用户在 config 里定义预设列表，一个键循环切换。

```toml
[layout]
preset_widths = [
    { proportion = 0.333 },
    { proportion = 0.5 },
    { proportion = 0.667 },
    { proportion = 1.0 },
]
# 也支持固定像素
# preset_widths = [{ fixed = 800 }, { proportion = 0.5 }]
```

一个键（如 `r`）向前循环，`Shift+R` 向后循环。当前选中的 preset index 记在 column 上。手动 resize 后 preset index 清除。

**灵感来源：** niri 的 `preset_column_widths` + `switch_preset_column_width()` 循环机制。
**差异化：** niri 是窗口管理器，preset 是给应用窗口用的。Ciri 的 preset 应该考虑终端场景——比如内置 "80 列" "120 列" 这类字符宽度的预设，而不只是比例。

#### 1.2 Column 内垂直堆叠（Consume / Expel）

**现状：** 1 column = 1 pane，没有垂直分屏能力。想上下排列两个 pane 只能用 workspace row（不在同一视口）。
**目标：** 一个 column 可以放多个 pane，上下排列，共享列宽。

```
┌──── col 0 ────┬──── col 1 ────┬──── col 2 ────┐
│               │    pane B     │               │
│   pane A      ├───────────────┤   pane E      │
│   (full)      │    pane C     │   (full)      │
│               ├───────────────┤               │
│               │    pane D     │               │
└───────────────┴───────────────┴───────────────┘
```

**数据模型变更：**
```rust
// 之前
struct Column { pane_id: PaneId, width: ColumnWidth }

// 之后
struct Column {
    tiles: Vec<Tile>,          // 多个 pane 上下排列
    active_tile_idx: usize,    // 当前 focus 在哪个 tile
    width: ColumnWidth,        // 所有 tile 共享列宽
}

struct Tile {
    pane_id: PaneId,
    height: TileHeight,        // Auto { weight } 或 Fixed(px)
}

enum TileHeight {
    Auto { weight: f64 },      // 按权重瓜分剩余高度（类似 flex）
    Fixed(f64),                // 固定像素高度
}
```

**核心操作：**
- **Consume**（Leader + `c`）：把右边 column 的 pane 吸入当前 column，变成上下堆叠
- **Expel**（Leader + `v`）：把当前 column 里的 active pane 弹出为右边的独立 column
- **Column 内导航**：Leader + `j/k` 在 column 内上下切换 focus（当 column 只有 1 个 pane 时，j/k 走 workspace row 导航）

**高度分配规则：**
- 默认所有 tile 用 `Auto { weight: 1.0 }`，等分高度
- 用户可以调整 weight 或设为 Fixed 像素
- Column 内拖拽上下边框调整相邻 tile 的高度比例

**跟 tmux 的本质区别：**
| | tmux | Ciri |
|---|---|---|
| 结构 | 递归二叉树，任意嵌套 | **两层：column → tiles，不嵌套** |
| 宽度 | 每个 pane 可以不同宽度 | **同一 column 内所有 tile 共享列宽** |
| 高度 | 像素级手动切分 | **weight 权重自动分配** |
| 操作 | split-window → 先分屏再启 shell | **consume/expel → 对已有 pane 重新编排** |

**使用场景：**
- 上面 vim 下面 terminal，放在一个 column 里，整体跟着视口滚动
- 临时 consume 一个日志 pane 进来对照看，看完 expel 出去
- 三个微服务日志上下堆叠在一个窄 column 里，旁边是宽的编辑器 column

#### 1.3 智能相邻 pane resize

**现状：** 放大一个 pane，相邻 pane 不受影响（只是被推走）。
**目标：** 提供"联动 resize"模式——放大左边，右边等比缩小，总宽度守恒。

这在两个 pane 并排可见时特别有用。用户拖动边框，两个 pane 的宽度此消彼长。

#### 1.3 居中策略可配

**现状：** 总是把 active column 居中。
**目标：** 三种策略可选。

```toml
[layout]
center_focused_column = "on-overflow"  # "always" | "on-overflow" | "never"
```

- `always`：当前行为
- `on-overflow`：只有 pane 比 viewport 宽时才居中，否则靠左对齐（更像传统编辑器体验）
- `never`：纯靠左排列

**灵感来源：** niri 的 `CenterFocusedColumn` 枚举。
**差异化：** 终端场景下 `on-overflow` 可能是最佳默认值——大部分终端 pane 都不会比屏幕宽。

---

### 第二阶段：终端复用器的刚需

这些不是从 niri 借鉴的，是终端场景的硬需求。

#### 2.1 Scrollback 搜索（Ctrl+F）

tmux 有 copy mode + `/` 搜索，Zellij 有搜索，Alacritty 有 Vi mode 搜索。Ciri 目前没有。这是最高优先级的缺失功能。

**设计：**
- `Ctrl+Shift+F` 进入搜索模式
- 输入关键词，实时高亮所有匹配
- `n/N` 跳转上/下一个匹配
- 搜索范围：当前 pane 的 viewport + scrollback
- 支持正则表达式

#### 2.2 Pane 联动输入（Broadcast input）

tmux 的 `synchronize-panes` 功能——同时向多个 pane 发送相同输入。运维场景必备。

**设计：**
- Leader + `b` 切换当前 workspace 的 broadcast 模式
- 所有可见 pane 同时接收键盘输入
- 状态栏显示 broadcast 指示器

#### 2.3 Shell Integration（语义化区域）

Ghostty 和 WezTerm 都支持 OSC 133 shell integration：
- 区分 prompt / command / output
- 支持"跳到上一个 prompt"导航
- 智能选择一整块命令输出

**差异化：** Ciri 可以让滚动式 layout 和 shell integration 联动——比如自动把每个命令的输出收缩成可展开的折叠块。

#### 2.4 远程 Session（SSH）

这是 tmux 的核心场景。Ciri 的 client-server 架构天然支持扩展到远程：
- 本地 client 通过 SSH tunnel 连接远程 server
- 远程 pane 获得本地 GPU 渲染能力
- 本地 pane 和远程 pane 可以混排在同一个 workspace 里

这是 Ghostty/Alacritty 做不到、tmux 做得丑的事情。

---

### 第三阶段：视觉与交互打磨

#### 3.1 Pane 打开/关闭动画

**现状：** 创建和关闭 pane 是瞬间完成。
**目标：** 打开 pane 有 fade-in + slide 动画，关闭有 fade-out 动画。

实现方式：关闭时捕获最后一帧的快照，对快照做 opacity 动画。这比 niri 的做法更简单，因为终端 pane 的内容不会像 GUI 窗口那样复杂。

#### 3.2 Focus 视觉增强

**现状：** Active pane 有 border 颜色区分。
**目标：** 可配置的 focus ring——宽度、颜色、圆角、不活跃 pane 的 dimming。

```toml
[appearance.focus_ring]
width = 2.0
active_color = "#89b4fa"
inactive_color = "#45475a"
corner_radius = 4.0

[appearance]
inactive_opacity = 0.85  # 不活跃 pane 半透明
```

**灵感来源：** niri 的 FocusRing + inactive_opacity。
**差异化：** Ciri 可以做 GPU 级别的效果（模糊、阴影），niri 也能但 tmux/Zellij 绝对不行。

#### 3.3 触控板手势

**现状：** 无手势支持。
**目标：** 三指左右滑动切换 pane，上下滑动切换 workspace row。

实现方式：winit 已经支持 touchpad gesture 事件。需要加一个 SwipeTracker 来追踪速度和惯性。

**灵感来源：** niri 的 SwipeTracker + 减速物理。
**差异化：** tmux/Zellij 永远做不了手势（它们跑在别人的终端里）。

---

### 第四阶段：生态差异化

#### 4.1 Layout 模板

Zellij 的 KDL layout 文件是个好主意，但 Ciri 可以做得更好：

```toml
# ~/.config/ciri/layouts/dev.toml
[[row]]
columns = [
    { command = "nvim .", width = { proportion = 0.6 } },
    { command = "cargo watch", width = { proportion = 0.4 } },
]

[[row]]
columns = [
    { command = "lazygit", width = { proportion = 1.0 } },
]
```

**差异化：** TOML 而非 KDL（Ciri 全家桶统一 TOML），且 layout 模板和 session restore 是一套系统——模板创建 session，session 可以被保存为新模板。

#### 4.2 命令面板（Fuzzy Finder）

类似 VS Code 的 Ctrl+P / Zellij 的 session manager：
- 快速切换/搜索 pane（按标题、CWD、运行命令）
- 切换 session
- 执行 action

在 pane 数量多的滚动式 layout 里，这比按 h/l 一个个翻要高效得多。

#### 4.3 Niri 原生集成

Ciri 跑在 niri 下时，可以利用 niri 的 IPC：
- Ciri 的 workspace row 映射为 niri 的 column
- 或者 Ciri 的 pane 直接映射为 niri 的 tile

这是只有 Ciri 能做的事情——因为它的 layout 模型跟 niri 高度同构。

---

## 不做什么

明确列出 Ciri 不会做的事情，避免功能蔓延：

| 不做 | 理由 |
|------|------|
| 浮动窗格（Zellij 的 floating pane） | Ciri 的滚动列模型已经解决了"临时看一眼"的需求——新建一个窄 column 然后关掉。浮动窗格会破坏滚动列的一致性。 |
| WASM 插件系统 | 过度工程化。Ciri 优先通过 IPC + CLI 提供可组合性，而不是内嵌运行时。 |
| 递归二叉树分屏（tmux 风格） | Ciri 支持 column 内垂直堆叠（两层结构），但不做任意嵌套。column → tiles 是终点，tile 内不能再 split。这保持了模型的简洁性。 |
| 平台原生 UI（Ghostty 风格） | Ciri 的核心价值在渲染和 layout，不在系统集成。跨平台一致性更重要。 |
| 内置 SSH client（WezTerm 风格） | 不把 SSH 实现塞进终端。通过 SSH tunnel + remote server 的方式更干净。 |

---

## 总结：Ciri = 滚动画布 + GPU 渲染 + Session 保活

| 维度 | tmux | Zellij | Ghostty | Ciri |
|------|------|--------|---------|------|
| 渲染 | ASCII（宿主终端） | ASCII（宿主终端） | GPU（原生） | GPU（wgpu） |
| Layout | 固定分屏 | 固定分屏+浮动 | Tab+Split | **无限滚动列** |
| Session | ✅ 远程保活 | ✅ 本地保活 | ❌ | ✅ 本地保活（远程 planned） |
| 动画 | ❌ | ❌ | 有限 | **弹簧物理** |
| 扩展性 | 脚本 | WASM 插件 | ❌ | IPC + CLI（planned） |

**Ciri 不是 "better tmux"，不是 "Rust 版 Zellij"，也不是 "niri 的终端版"。它是一个新品类：滚动式终端工作空间。**
