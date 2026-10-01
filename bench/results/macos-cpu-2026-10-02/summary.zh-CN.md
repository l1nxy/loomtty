# macOS TUI CPU 优化 · 2026-10-02

通过当前版本采样定位原生状态重复同步，并使用三轮交错对照验证。每次 20 阶段，优化前后各 3 次，共 120 个有效阶段。

[完整报告](report.md) · [原始数据](results.json) · [日志](logs/) · [采样](profiles/) · [代码补丁](implementation.patch)

## CPU

单位为单核百分比，Loomtty 包括客户端和私有服务端。中位数后括号为最小–最大值，不是置信区间。

| 场景 | 优化前 | 优化后 | 变化 |
|---|---:|---:|---:|
| 全屏 TUI / 合计 | 13.38 (12.93–13.41) | 12.33 (12.21–12.34) | -7.9% |
| 全屏 TUI / 客户端 | 9.26 (8.95–9.26) | 8.19 (8.17–8.22) | -11.6% |
| 全屏 TUI / 服务端 | 4.12 (3.98–4.15) | 4.12 (4.04–4.14) | -0.1% |
| 单行 TUI / 合计 | 7.40 (7.31–7.55) | 6.45 (6.33–6.50) | -12.9% |
| 单行 TUI / 客户端 | 6.07 (5.94–6.15) | 5.09 (4.98–5.11) | -16.1% |
| 单行 TUI / 服务端 | 1.38 (1.33–1.41) | 1.35 (1.35–1.38) | -1.8% |

各组件分别取中位数，不能保证相加等于总量中位数。每组优化后的全屏和单行 CPU 都低于对应基线。服务端未做逻辑改动，其小幅波动不视为优化收益。

## 改动与采样证据

优化前全屏与单行客户端调用栈都出现 `MacApplication::update_native_state`、`Bridge::capture` / `update_automation`，同时存在行重建、面板字形组装和 Metal 提交开销。采样是包含等待的调用栈样本，不能按样本数直接换算函数 CPU 百分比。

- 纯 PTY 通知与 RedrawRequested 不再多次同步相同原生状态，统一由同一事件循环末尾的 about_to_wait 完成。菜单等命令仍在分派前后同步；焦点、输入、窗口事件仍立即同步，包括 Secure Input。
- 普通终端窗口复用已解析的快捷键映射，仅 Settings 或无终端窗口时构建备用映射。
- AppleScript / App Intents 在完成等待状态检查后直接移交快照，避免深拷贝整个对象图；仍先发布新快照，再恢复等待中的脚本命令。

未降低目标帧率、增设节流延迟、关闭原生功能或改变负载内容；本轮也没有改 GPU shader、渲染分辨率或服务端算法。

## 响应与限制

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| 全屏 DSR p95，ms | 0.524 | 0.513 |
| 单行 DSR p95，ms | 0.156 | 0.157 |

所有 12 个 TUI 阶段均为零次解析确认超期。DSR 只确认服务端解析，不测 GPU 实际呈现、FPS 或按键到像素延迟。剩余客户端开销仍包含字形处理、整帧数据组装与 Metal 提交，本次优化没有消除这些成本。

## 条件与验证

Apple M6、32 GiB、macOS 27.0、交流电；100×30、Menlo 10pt，1× DPI、6×12 单元格，600×400 logical pixels。基线为 main `b3216b58f094cf75b32b312af85c7829525ae7f0` 的 release + macos-app-intents，精确二进制哈希见原始 JSON。

前后顺序为 before/after → after/before → before/after。六类 bulk 各至少 8 MiB；全屏/单行 TUI 各 600 次、60 Hz、约 10 秒。CPU 包含 250 ms 尾段；不含 worker、shell、WindowServer 或 GPU 执行时间。前后网格、DPI、bulk 字节数/哈希、TUI 帧数/字节数一致。没有运行期间的 panic、DPI/config 变化或脚本检测到的 cargo/rustc 干扰；无法排除所有后台负载。

387 项客户端测试通过、1 项忽略；cargo fmt、cargo clippy --workspace --all-targets 和 release 构建完成，Clippy 保留现有 warnings。采样运行与正式性能运行分开，不混入统计。

## 本机验证

已安装优化版，保留原服务端和 slim-bear 的两个面板。通过原生菜单、Cmd+, 打开 Settings、Cmd+W 关闭 Settings 验证了菜单禁用状态及返回终端后的恢复；没有修改用户配置或关闭已有面板。

## 复跑

```sh
python3 bench/compare_loom.py --before /tmp/loom-bench-build-20261001 \
  --loom target/release/loomtty --loom-server target/release/loomtty-server \
  --rounds 3 --frames 600 --loom-width 600 --loom-height 400 \
  --output /tmp/loom-tui-compare-new
```
