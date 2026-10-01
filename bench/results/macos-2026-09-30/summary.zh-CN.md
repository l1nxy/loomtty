# macOS 终端性能对比 · 2026-09-30

> 后续日志审计发现：三次 loomtty 运行都出现了配置监听线程 panic，退出时也有清理 panic。
> 下文“成功”仅指工作负载完成，不能视为无错误的性能基线。数字保留供追溯，优化结论需使用
> 修正配置目录隔离和错误检测后重新采集的基线。其他程序未发现同类日志错误。
> [重新采集的优化前后对照](../macos-performance-2026-09-30/summary.zh-CN.md)已通过日志审计。
> 四终端比较请使用[新版报告](../macos-comparison-2026-09-30/summary.zh-CN.md)。

完成了 loomtty、Alacritty、Kitty、Ghostty 各 3 次独立运行，共 12 次、240 个阶段，全部成功。
各程序收到的测试数据 SHA-256 一致，实际 PTY 网格均为 100×30。

本次结果中，Alacritty 的多数批量输出场景领先；loomtty 的空闲内存已接近其他终端，
但解析响应延迟明显偏高，值得优先排查服务端的 16 ms 节流。
这是当前配置和本机上的测量，不能据此给出适用于所有负载的综合排名。

## 环境与版本

- Apple M6，32 GiB 内存，macOS 27.0，arm64，接交流电。
- Menlo 10 pt，100×30 cells，不透明窗口，关闭光标闪烁、连字和工作负载的 shell integration。
- loomtty 0.1.0：当前 `codex/new-branch` release 构建，基于 `fe438ab`，包含工作区尚未提交的 macOS 优化。
- Alacritty 0.17.0 (`94e7c88`)：官方源码原生 release 构建，安装到 `/Applications/Alacritty.app`。
  Homebrew cask 已停用，因此使用官方 `make app` 构建安装，没有修改 Gatekeeper 设置。
- Kitty 0.49.1：Homebrew cask 安装。
- Ghostty 1.3.1：使用本机已有 stable 安装。

每轮按固定随机种子打乱程序顺序，每款程序使用独立的新进程和配置。
以下为三次运行的中位数；loomtty 的资源数字包含 **GUI 客户端 + 私有服务端**。

## 内存与 CPU

| 指标 | loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 初始空闲 RSS，MiB | 106.6 | 101.4 | 123.4 | 116.5 |
| 初始空闲 physical footprint，MiB | 63.4 | 46.5 | 64.3 | 74.4 |
| 字形预热后 RSS，MiB | 114.0 | 107.9 | 132.3 | 125.0 |
| 输出历史记录后 RSS，MiB | 162.0 | 135.2 | 167.4 | 134.2 |
| 输出历史记录后 physical footprint，MiB | 118.2 | 78.2 | 103.5 | 95.3 |
| 字形预热后空闲 CPU，单核 % | 0.01 | 0.05 | 0.02 | 0.72 |
| 60 Hz 全屏 TUI CPU，单核 % | 18.1 | 10.5 | 14.7 | 16.0 |
| 60 Hz 单行 TUI CPU，单核 % | 8.6 | 10.9 | 5.2 | 7.5 |

RSS 与 physical footprint 是不同的内存口径，不应混用。loomtty 两个进程的 RSS 相加可能重复计入共享页。
历史缓冲限制也有差异：前三款设置为 10,000 行，Ghostty 设置为包含活动屏幕的 10,000,000 字节；
历史记录后的内存不能当成完全相同保留容量下的比较。

CPU 使用 `proc_pid_rusage` 采样，并通过 `mach_timebase_info` 将 Apple Silicon 的 Mach ticks 转为秒，
已与 Python 的进程 CPU 时钟交叉验证。CPU 包含每项结束后 250 ms 的渲染等待尾段，不包括工作负载 Python 和 WindowServer。
本次全屏 TUI CPU 的逐轮范围为：loomtty 18.05–18.17%、Alacritty 6.19–10.80%、Kitty 14.63–14.91%、Ghostty 7.41–16.59%。
部分程序的 CPU 波动较大，不宜把中位数附近的小差异解释为稳定优势。

## PTY + 解析吞吐

每项至少 8 MiB，另有独立的 64 KiB 预热；单位 MiB/s，越高越好。
计时结束前等待终端的 DSR 响应，确认解析到达数据末尾。

| 场景 | loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| ASCII 长行 | 87.7 | 117.7 | 108.3 | 79.1 |
| 逐字符真彩色 SGR | 90.5 | 110.0 | 107.0 | 80.1 |
| 中文 | 94.2 | 110.1 | 110.7 | 86.6 |
| Emoji | 100.1 | 108.8 | 107.2 | 91.4 |
| 短行密集滚动 | 80.8 | 103.9 | 48.2 | 67.4 |
| 光标定位/擦除/重绘 | 83.6 | 109.2 | 108.0 | 79.7 |

loomtty 的 ASCII 吞吐约为 Alacritty 的 74.5%；本次六类吞吐均高于 Ghostty，短行滚动也高于 Kitty。
吞吐包含 PTY 传输和驱动开销，不是纯解析器微基准，更不是 GPU 帧率。

## 解析响应与 TUI 更新

单位 ms，越低越好。分位数先在每轮内计算，再取三轮中位数。

| 指标 | loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 空闲 DSR p50 | 11.925 | 0.143 | 3.527 | 0.116 |
| 空闲 DSR p95 | 12.938 | 1.097 | 3.577 | 0.160 |
| 32 KiB 输出后 DSR p95 | 1.332 | 0.563 | 3.880 | 0.828 |
| 32 KiB 输出后 DSR p99 | 18.396 | 0.768 | 3.989 | 0.985 |
| 全屏 TUI 更新 DSR p95 | 16.939 | 0.387 | 3.379 | 0.636 |
| 单行 TUI 更新 DSR p95 | 17.081 | 0.269 | 6.066 | 0.286 |
| 全屏 TUI 超过解析响应期限的次数 / 300 | 24 | 0 | 0 | 0 |

loomtty 的全屏 TUI 三轮分别出现 29、24、20 次解析响应超期；空闲 DSR 的三轮结果也稳定在约 12 ms。
`crates/loom-server/src/daemon/tick.rs` 的 tick loop 把 PTY 处理和画面更新放在同一循环中，并有 16 ms 节流。
它与当前延迟特征吻合，是**待验证的原因**；应通过解耦解析/应答与画面发送后重跑同一基准验证，不能仅凭相关性确认。

DSR 是 **PTY/终端解析往返**。loomtty 由服务端应答，因此这些数值不包含 GUI 呈现确认。
上表超期次数不是实测掉帧数，也不能把 DSR 当成按键到屏幕的延迟。后者需要单独的显示呈现或相机测量。

## 启动与测量边界

启动到首次 DSR 应答的中位数：loomtty 813 ms、Alacritty 813 ms、Kitty 924 ms、Ghostty 545 ms。
范围分别为 508–852、560–822、586–1012、529–852 ms，启动抖动较大。
这包含 Python 工作进程启动，loomtty 还包含服务端启动；系统文件缓存保持温热，不是冷启动测试。

批量负载在 alternate screen 中运行，以免历史存储策略决定吞吐结果。
同样的字体配置和字符网格不保证相同的物理像素尺寸、fallback 字体、UI chrome 或渲染策略。
本次使用真实桌面环境，没有自动核验每个采样时刻的窗口遮挡状态，CPU/呈现方面的结论应保留这一限制。
没有测量 GPU 执行时间、真实显示帧率、图像协议、大量分屏或输入到显示的完整延迟。

## 复跑与审计

```sh
cargo build --release -p loomtty -p loomtty-server
python3 bench/macos_bench.py --rounds 3 --output /tmp/terminal-bench-new
```

- [完整自动生成报告](report.md)：全部指标和吞吐/启动范围。
- [原始结果](results.json)：每轮配置、样本、CPU/内存分项、负载 hash、二进制及脚本 hash。
- [终端日志](logs/)：仅本次测试实例的日志。
- [脚本与方法说明](../../README.md)。

验证：6 项 Python 测试、162 项相关 Rust 测试、`cargo fmt --check`、
`cargo clippy --workspace --all-targets` 均通过；Clippy 仍有仓库已有警告。
测试启动的进程均已退出，用户原有的 Ghostty 实例仍保留。
