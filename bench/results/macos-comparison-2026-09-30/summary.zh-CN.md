# 四终端性能对比 · 2026-09-30

loomtty、Alacritty、Kitty、Ghostty 各完成 3 次有效独立运行，共 240 个阶段。表格为中位数；[全量指标与范围](report.md)、[原始数据及失败记录](results.json)、[日志](logs/) 已保存。

本次比较使用完成调度/扫描优化的 loomtty。其[优化前后对照](../macos-performance-2026-09-30/summary.zh-CN.md)单独采集，避免把不同终端之间的差异解释成这次代码改动的收益。

## 测试环境与安装

- Apple M6，32 GiB，macOS 27.0，交流电。100×30 cells、Menlo 10 pt，不透明窗口，关闭光标闪烁和连字。loomtty 日志验证为 1× DPI / 8×16 cell / 800×512 logical pixels。
- loomtty 0.1.0：`codex/new-branch`，基于合入 macOS 修复的 `fe438ab`，含未提交的本地优化；资源包括客户端和私有服务端。
- Alacritty 0.17.0：官方固定版本源码 release 构建，安装到 `/Applications/Alacritty.app`。Kitty 0.49.1：Homebrew cask 安装。Ghostty 1.3.1：使用本机已有安装。
- 六类 bulk 每类至少 8 MiB，独立预热；DSR 各 200 次；全屏/单行 TUI 各 300 次、目标 60 Hz；三段 idle 各 5 秒；另测 10,000 行历史。所有有效运行的网格、bulk 数据字节数与 SHA-256 一致。

## PTY + 解析吞吐（MiB/s，越高越好）

| 场景 | loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| ascii | 217.5 | 199.3 | 208.9 | 123.6 |
| truecolor | 257.8 | 176.7 | 96.2 | 119.7 |
| cjk | 268.4 | 181.7 | 176.0 | 121.0 |
| emoji | 294.6 | 190.2 | 188.5 | 124.6 |
| scroll | 185.1 | 201.7 | 62.4 | 67.7 |
| cursor | 213.4 | 199.8 | 165.6 | 121.4 |

## 内存、CPU 与 DSR 响应

| 指标 | loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 预热后 RSS，MiB | 107.8 | 104.0 | 116.5 | 122.1 |
| 预热后 physical footprint，MiB | 57.5 | 38.7 | 42.9 | 55.9 |
| 预热后空闲 CPU，单核 % | 0.01 | 0.10 | 0.06 | 0.16 |
| 全屏 TUI CPU，单核 % | 14.9 | 9.2 | 2.8 | 15.0 |
| 单行 TUI CPU，单核 % | 5.9 | 8.2 | 0.3 | 6.8 |
| 空闲 DSR p95，ms | 0.128 | 0.173 | 3.896 | 0.140 |
| 全屏 TUI DSR p95，ms | 0.756 | 0.269 | 3.282 | 0.546 |
| 单行 TUI DSR p95，ms | 0.184 | 0.215 | 190.086 | 0.183 |

## 解读与限制

这些数字描述本机、此配置及此桌面状态下的测试，不给出通用综合排名。loomtty 在多数 bulk 场景有较高吞吐，但其服务端直接回复 DSR，客户端呈现可稍后发生，不能将此优势解释为屏幕更快。

Kitty 三次单行 TUI 都出现约 190 ms 的响应长尾，300 次更新约耗时 28 秒。缺乏窗口遮挡/App Nap 状态的独立记录，不能判定是应用自身还是后台调度导致。保留全部样本；尤其不要把它该阶段较低的平均 CPU 当成在相同更新节奏下更高效。其他应用的 CPU 同样不代表 GPU 呈现时间。

- DSR 只确认 PTY 输送与解析完成；这些测试不测 FPS、按键到像素延迟或每个中间帧是否显示。TUI deadline miss 是解析确认超期，不是屏幕掉帧。
- CPU 相对单核，可超过 100%，按 Mach timebase 转为秒，并含阶段后 250 ms 尾段。排除 workload Python、shell、WindowServer 和 GPU 执行时间。
- RSS 与 physical footprint 不同，loomtty 双进程 RSS 可能重复计算共享页。历史存储上限也不同：前三款 10,000 行，Ghostty 为包含活动屏幕的 10 MB；不能当成完全相同历史保留量。
- 原始三轮随机顺序运行中，有 Rust 后台构建干扰和 Ghostty 的 LaunchServices 首窗启动超时；均标记失败并排除。之后只补缺失的 Kitty/Ghostty 运行，故最终数据并非完整连续的三轮随机交错。`source_batches` 保留每批设置、脚本哈希及原始元数据。
- Ghostty 后改为直接启动原生应用可执行文件，新进程独立创建窗口并运行相同工作负载；不会把命令发送给用户已有会话。每轮仍是独立新进程。
- 每阶段前后检测 cargo/rustc，并审计异常日志。无法排除所有背景活动、采样间隙内的短构建、窗口遮挡或温度变化。三次样本不是置信区间。旧 2× DPI 报告及异常 loomtty 数据不混入此次统计。

## 复跑

```sh
bash bench/install_macos.sh
cargo build --release -p loomtty -p loomtty-server
python3 bench/macos_bench.py --rounds 3 --loom-height 512 \
  --output /tmp/terminal-bench
```

请在桌面会话中保持测试窗口可见，暂停其他构建；2× 显示器请先用 `--probe` 校准，默认 loomtty 高度 496。脚本、测量边界和更多选项见 [bench 使用文档](../../README.md)。
