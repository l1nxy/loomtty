# 滚屏候选验证汇总

基线是 main d60c18a；客户端始终是同一个签名后二进制，仅私有服务端替换 alacritty_terminal 候选。原 fork 基线 867784628f619fc2f58c55d271ad5f6014d722c6；最新版候选 v3 = 15f9b6f1，合并差异见 candidate-v3.patch，二进制哈希见 binaries-v3.json。未合入 main、未推送或安装。

所有定向场景均三对新进程，顺序 before/after、after/before、before/after。100×30、Menlo 10pt、6×12 cell、DPI 1。上游 vtebench ead80032e57dee2e75f0b51f2ea67528647d9944，16 MiB 单次输入、1 次 warmup、3 秒采样，保留 PTY OPOST|ONLCR（flags=3）。CPU 是客户端+服务端，按包括 warmup 的输入总 MiB 归一化；分母为 PTY 转换前的上游字节。DSR 不代表客户端呈现完成，不是 FPS 测试。

测试按用户要求 CLI 自动前台启动，无人工前台门禁，未连续监视遮挡。所有纳入结果的运行都通过了前后并发 Rust 构建检查、启动尺寸和输入/输出标志一致性检查；这不是“整个系统完全无后台活动”的保证。较早被并发构建打断的批次没有混入这些对照。

## v3 结果（各组三次启动的中位数）

| 场景 | 吞吐 before → after MiB/s | CPU before → after ms/MiB |
|---|---:|---:|
| scrolling | 82.69 → 100.00 | 16.54 → 14.34 |
| scrolling_bottom_region | 96.39 → 95.24 | 14.32 → 14.47 |
| scrolling_bottom_small_region | 55.36 → 54.79 | 22.70 → 23.00 |

普通 scrolling 吞吐约 +20.9%，CPU/输入量约 -13.3%，三对方向一致。底部两种区域仍有约 1% 吞吐代价、约 1.0–1.3% CPU/输入量代价，不能写成“所有场景无退化”。原始结果和 .dat 位于 v3 各场景子目录。

## 完整 Loom A/B

另做三对默认完整 runner（6 项 bulk、idle、输入延迟、600 帧/60 Hz 全屏和局部 TUI、10,000 行历史），相同 main GUI、分别使用基线/候选服务端，6 次全部完成。见 v3/full-ab/report.md 与 results.json。

| 指标 | before | after |
|---|---:|---:|
| 全屏 TUI CPU（单核 %） | 12.4379 | 12.5742 |
| 局部 TUI CPU（单核 %） | 8.7127 | 8.6753 |
| 历史后 physical footprint MiB | 132.408 | 132.345 |
| 全屏 TUI DSR p95 ms | 0.592 | 0.689 |
| 全屏 TUI DSR p99 ms | 0.714 | 0.880 |
| 局部 TUI DSR p95 ms | 0.390 | 0.293 |
| loaded latency DSR p95 ms | 0.184 | 0.190 |

本候选没有 TUI CPU 或内存改善证据。全屏 TUI 尾延迟数值偏高，不能宣称延迟无退化；各 TUI parser deadline miss 为 0，也不能据此宣称相同显示帧率。

## 版本与取舍

v1（1b584fcb）把完整快路径放进 Grid::scroll_up 的可内联入口：普通滚屏吞吐 +21.6%、CPU/MiB -12.4%，但底部区域约 2.9% 吞吐、3.4% CPU/MiB 退化。六种上游滚屏场景、每项三对保存于 v1。

v2（37f53bf3）把判断移到 Term 的区域处理处，Grid 通用入口恢复原样，但编译器仍把快路径内联进公共调用者；底部区域约 2.4% 吞吐代价仍在。三种场景每项三对保存于 v2。

v3 将完整快路径保持为独立函数。相同三场景中底部代价缩小至约 1%，并保留普通滚屏收益。暂时保留为候选，完整四终端比较和剩余 v3 上游场景还未完成，因此尚未合入或发布。候选并未解决所有 TUI/内存问题。

## 正确性与剩余工作

终端库差分测试比较整屏单行快路径与原始通用算法，覆盖历史增长、回绕、模板变化、视口滚动和计数饱和。终端库 137 项测试通过；Loom 终端、协议、数据模型和跨 crate 集成测试共 549 项通过、3 项忽略。workspace clippy、release 构建完成；变更保持在独立 worktree，主工作区 Cargo 依赖未改。

还需完成 v3 的 scrolling_fullscreen、scrolling_top_region、scrolling_top_small_region 回归，以及三轮完整四终端（Loomtty / Alacritty / Kitty / Ghostty）对照。不得用 v1 的数据冒充 v3，也不得把这些定向 A/B 算成已完成四终端对比。对已观察到的区域/TUI 延迟代价，最终选择时必须如实保留，不只筛选获益场景。
