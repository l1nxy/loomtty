# 四终端 vtebench 首轮对照（raw PTY）

本轮继承 DSR 驱动的 raw PTY 输出设置，OPOST 关闭；上游 harness 自身不修改 termios。此处是相同 raw 条件的对照，不能与默认命令行输出模式混用。后续需要按保存的原始输出设置复测 vtebench；TUI/历史阶段不受这个修正影响。

四款均完成 12 项官方负载，setup/payload 哈希完全一致。使用未修改的 alacritty/vtebench，commit `ead80032e57dee2e75f0b51f2ea67528647d9944`。100×30、Menlo 10 pt；TUI 每项 600 帧、60 Hz，均约 10 秒且 missed_deadlines=0。

每款目前只有一次成功启动，分开顺序运行，未随机化，也未做重复轮次统计。不要把小差异当成稳定排名。Loomtty 由 CUA 确认前置；其余三款由用户逐个确认，收据保存于各应用目录。没有持续遮挡监测。原始 results.json 保持不变，.dat 只去除行尾空白。

| 指标 | Loomtty | Alacritty | Kitty | Ghostty |
|---|---:|---:|---:|---:|
| 全屏 TUI CPU（单核 %） | 12.95 | 9.13 | 18.09 | 14.71 |
| 局部 TUI CPU（单核 %） | 8.49 | 8.26 | 8.90 | 6.35 |
| 预热后 footprint（MiB） | 68.17 | 46.30 | 63.24 | 82.36 |
| 历史后 footprint（MiB） | 129.55 | 72.34 | 97.50 | 90.77 |

Loomtty 统计 GUI+私有服务端，其他统计应用进程。排除 workload、WindowServer 和 GPU 执行时间。footprint 与 RSS 分开；历史策略为 10,000 行，Ghostty 是 10 MB 原生容量，不能视为等量历史内存排名。DSR 是解析/PTY 回应，不是 input-to-photon 或呈现 FPS。

## 下一步依据

- 单轮全屏 TUI：Loomtty 高于 Alacritty，低于本轮 Kitty、Ghostty；局部 TUI 仍高于 Ghostty。不能继续笼统说“比其他终端都高”。之前 Kitty 局部刷新被拖长的旧结果不混用。
- 官方 scrolling：Loomtty 28.2 MiB/s、40.28 CPU ms/MiB；Alacritty 36.6、28.36；Kitty 17.9、56.62；Ghostty 82.9、15.50。Loomtty 在该阶段服务端 CPU 4.298 s，客户端 0.214 s，约 95% CPU 来自服务端。下一项 profile 针对这条已测得的瓶颈。
- Loomtty 的 dense_cells / medium_cells 等洪泛负载并不落后于其余三款，不能用一个场景代表所有性能。
- 当前被测 Loomtty 含已验证的客户端直接解码历史优化；服务端仍为 e70f2385，不含 69f0097 候选编码优化。候选使锁内快照变慢，因此仍未合并或安装。
- 仍需对选定优化做未插桩重复 A/B 和回归检查；本报告不是整个优化任务完成声明。

## 运行与版本

每款 source JSON 包含启动时刻、系统/供电状态、完整参数、二进制与脚本 SHA。各次脚本差异仅为 Kitty/Alacritty/Ghostty 的测试窗口标题；所有实际输入 manifest 与公共测试设置已程序核对一致。

Ghostty 首次使用 CLI 标题参数时未收到 ready，运行在准备阶段超时；随后将标题移入私有 config 后成功。失败尝试未纳入数据。用户原有会话未被操作，各正式测试的私有实例由 runner 回收。

[全部指标](report.md) · [官方源码](https://github.com/alacritty/vtebench)
