# 四终端复测 · 2026-10-01

四款各 3 次有效独立运行，共 240 个阶段；三轮连续按固定种子打乱顺序，没有补跑或排除正式样本。以下为中位数，更多指标及吞吐、启动时间范围见 [完整报告](report.md)，[原始数据](results.json)、[运行日志](logs/)、[脚本快照](scripts/) 一并保存。

## 测试条件

- Apple M6、32 GiB、macOS 27.0，交流电，无系统记录的温度或性能警告。
- Loomtty 0.2.1，当前 main `b3216b58f094cf75b32b312af85c7829525ae7f0`，release + macos-app-intents；客户端与私有服务端均计入资源占用。Alacritty 0.17.0、Kitty 0.49.1、Ghostty 1.3.1。
- 全部 100×30 cells、Menlo 10 pt。Loomtty 原生窗口校准为 600×400 logical pixels，日志确认 1× DPI、6×12 像素单元格，三轮一致。原 2026-09-30 比较的 Loomtty 使用 8×16 单元格；期间字号和窗口布局已修正，不能将两份报告直接当作同条件优化前后对照。
- 原脚本未改动：六类 bulk 各至少 8 MiB；每类先预热 64 KiB；两类 DSR 各 200 次；全屏/单行 TUI 各 300 次、目标 60 Hz；三段空闲各 5 秒；另输出 10,000 行历史。
- 校准失败与中止批次独立保存在 [calibration](calibration/README.md)，没有混入正式统计。

## PTY + 解析吞吐

单位 MiB/s，越高越好。

| 指标 | Loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| ascii | 238.4 | 201.1 | 209.6 | 116.3 |
| truecolor | 289.6 | 174.7 | 98.7 | 115.9 |
| cjk | 294.9 | 179.8 | 173.0 | 116.6 |
| emoji | 323.5 | 185.5 | 189.3 | 121.0 |
| scroll | 204.1 | 200.0 | 62.0 | 67.9 |
| cursor | 236.9 | 196.3 | 164.8 | 112.9 |

## 资源与响应

| 指标 | Loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 预热后 RSS，MiB | 115.1 | 105.6 | 118.3 | 124.4 |
| 预热后 physical footprint，MiB | 68.5 | 45.6 | 53.2 | 66.8 |
| 预热后空闲 CPU，单核 % | 0.01 | 0.07 | 0.06 | 0.14 |
| 全屏 TUI CPU，单核 % | 12.8 | 8.5 | 2.9 | 14.4 |
| 单行 TUI CPU，单核 % | 7.1 | 8.2 | 0.3 | 6.3 |
| 历史输出后 RSS，MiB | 191.5 | 132.5 | 152.1 | 133.7 |
| 历史输出后 physical footprint，MiB | 144.8 | 69.8 | 87.0 | 76.0 |
| 空闲 DSR p95，ms | 0.113 | 0.189 | 3.887 | 0.138 |
| 满载 DSR p95，ms | 0.179 | 0.225 | 5.233 | 0.517 |
| 全屏 TUI DSR p95，ms | 0.534 | 0.284 | 3.250 | 0.518 |
| 单行 TUI DSR p95，ms | 0.159 | 0.191 | 190.720 | 0.166 |
| 单行 TUI 实际耗时，s | 5.01 | 5.00 | 24.47 | 5.00 |
| 启动至首个 DSR，ms | 951.0 | 676.6 | 704.7 | 886.2 |

## 解读

Loomtty 六项 bulk 的中位数均最高，但滚动项与 Alacritty 接近。预热后的 RSS 位于 Alacritty 与 Kitty/Ghostty 之间；physical footprint 则略高于 Ghostty。全屏 TUI CPU 高于 Alacritty，单行 TUI CPU 低于 Alacritty、高于 Ghostty。历史输出后的 RSS 与 physical footprint 都是四款中最高，是本次可见的内存短板。

Kitty 三轮单行 TUI 耗时分别约 24.30、24.88、24.47 秒，目标为约 5 秒；p95 约 190.7 ms，确认超期次数中位数 118。脚本没有独立记录窗口遮挡/App Nap 状态，不能判断长尾来自应用内部还是后台调度。尤其不能把其 0.3% 平均 CPU 当作相同更新节奏下的效率优势。

DSR 仅确认 PTY 输送和终端解析；Loomtty 由服务端回复，不能将吞吐或 DSR 优势解释成更高 FPS 或更低按键到像素延迟。CPU 按单核计、包含阶段后 250 ms 尾段，排除 worker/shell、WindowServer 和 GPU 执行时间。RSS 可能重复计算 Loomtty 双进程共享页，因此也列出 physical footprint。历史容量策略不同：前三款按 10,000 行，Ghostty 为包括活动屏幕的 10 MB。

## 验证与复跑

12 次网格均为 100×30，六类负载的字节数与 SHA-256 跨全部样本一致；每次 20 个阶段。未发现 panic、配置重载、DPI 变化或脚本检测到的 cargo/rustc 干扰。9 项 benchmark 自测通过；全部测试拥有的客户端和私有服务端均已退出，未关闭用户现有终端。二进制构建参数和哈希见 [build.json](build.json)。脚本不能排除所有后台活动，3 次样本的范围也不是统计置信区间。

```sh
cargo build --release -p loomtty -p loomtty-server --features loomtty/macos-app-intents
python3 bench/macos_bench.py --rounds 3 --loom-width 600 --loom-height 400 \
  --output /tmp/terminal-bench-new
```

上述尺寸适用于本次 1× 显示环境；更换屏幕/DPI 后先运行 `--probe --rounds 1`。
