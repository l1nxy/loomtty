# 四终端：保留上游 PTY 输出模式的首轮对照

四款各完成一次启动、12 项上游 vtebench，以及附加 TUI/延迟/历史场景。所有上游输入 manifest 相同，PTY 输出实际为 flags=3（OPOST、ONLCR），100×30、Menlo 10pt。原始结果与 .dat 保存于各终端子目录。

这是一轮分开顺序执行的对照，Loomtty 数据早于另外三款；不是三轮随机交错实验，不能据此宣称稳定排名。Kitty 人工确认前置；用户之后明确要求不再逐窗确认，Alacritty/Ghostty 按用户观察到的启动自动前台行为运行。Loomtty 使用 CUA 核对窗口；没有连续监控遮挡。

上游 vtebench 不修改 termios；runner 仅在其子进程执行期间恢复 PTY 原输出标志。raw 旧结果不能与这里的吞吐量混排。Loomtty scrolling 从 raw 的 28.2 变成 82.1 MiB/s 是条件修正，不是优化收益。

| 指标 | Loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 全屏 TUI CPU（单核 %） | 11.91 | 5.93 | 13.04 | 13.33 |
| 局部 TUI CPU（单核 %） | 7.93 | 6.55 | 9.01 | 6.98 |
| Warm footprint（MiB） | 68.20 | 46.61 | 64.16 | 85.55 |
| 历史后 footprint（MiB） | 129.92 | 72.55 | 98.14 | 92.24 |
| scrolling（MiB/s） | 82.05 | 76.19 | 16.80 | 59.04 |
| scrolling（CPU ms/MiB） | 16.70 | 13.41 | 61.64 | 26.74 |
| scrolling_fullscreen（MiB/s） | 173.93 | 179.79 | 166.68 | 126.99 |
| scrolling_fullscreen（CPU ms/MiB） | 8.15 | 5.97 | 6.10 | 12.38 |

Loomtty CPU/内存包含客户端与独立服务端；其余包含各自 GUI。TUI 每项 600 次、60 Hz、约 10 秒，四款解析期限 miss 都为 0；DSR 完成不代表 GPU 呈现完成。footprint 排除生成器及 WindowServer，历史容量使用各产品原生限制（10,000 行 vs Ghostty 10 MB），不等同相同内容驻留。完整 12 项见 report.md。

## 本轮定位与候选状态

修正条件的 scrolling Time Profiler：26,560 ms 活跃权重。Grid::scroll_up 自身 27.78%、scroll_up_relative 自身 18.57%、PTY read 23.12%；Row::reset 6.37%，build_scrollback_only_sync 0.16%。父子 inclusive 占比不可相加。此前 raw 模式 Row::reset 的 51.60% 不能用于默认输出条件的优化优先级。

当前客户端全屏/局部 TUI 各 40 秒，分别附加 20 秒 Time Profiler。活跃权重 2,037 / 1,495 ms：draw_frame inclusive 14.63% / 19.20%；全屏 update_view_from_grid inclusive 15.86%、apply_delta_borrowed 7.85%。原始 trace 保留本机，仓库只归档去环境信息的调用栈及分析。

帧计数诊断（独立开启 trace 日志）在每项 600 次更新中得到全屏 567 / 局部 569 次 GPU 提交，空闲 0；没有重复提交证据。此计数不等同显示器 FPS，不用于 CPU A/B。

新增客户端候选：空附加字符/链接表不再对每个覆盖单元执行 hash remove；非空时仍清理覆盖范围，并保留历史、未覆盖字符和链接。289 项 loom-app 测试、release 构建、workspace clippy/fmt 通过。正式交错 A/B 期间 Mac 再次锁屏，整批 /tmp/loom-empty-extras-ab 已标为无效并中止。尚无有效收益数据，候选未合入 main、未安装或推送。

## 下一步

解锁后自动前台继续候选三轮交错 A/B；不再逐窗口请求确认。检查 CPU、内存、相同工作量和提交帧数，再决定保留或撤回。若保留，补最新构建的四终端交错复测。

## 2026-10-03 更新

空表清理候选已完成三轮有效 A/B，未证明稳定 CPU/内存收益，已撤回。见 [逐轮结果与方法](../../macos-empty-extras-2026-10-03/notes.zh-CN.md)。旧 /tmp 原始 trace 已被系统清理；这里只保留当时归档的调用栈、分析和记录脚本。启动计时的跨进程时钟问题已修正，旧数据的启动时间不用于比较。
