# 整屏单行滚动快路径：阶段性验证，尚未合入 main

当前最新候选为 v3（15f9b6f1），已完成三种上游场景和完整 Loom 三对 A/B。最新结论见 [验证汇总](validation.zh-CN.md)。以下保留最初准备过程；其中“尚无收益结论”等历史状态已被后续 v1/v2/v3 数据更新。

## 依据与限制

10 月 2 日默认 PTY 输出模式的有效 server-scrolling profile 中，Grid::scroll_up 自身为 27.78%，scroll_up_relative 自身为 18.57%。因此本次检查此路径，而非再优化只占很小比例的同步编码。

本次对 main d60c18a 独立签名 bundle 做了 20 秒 Time Profiler，工作负载为同一个官方 vtebench scrolling，保留原始 PTY 输出标志，100×30。测试窗口已由 agent 自动 Raise、AX 和截图核对。30 秒工作负载结束检查检测到其他 Rust 构建（failure.json），因此本次吞吐量/CPU 数据不得作为正式对照或收益基线。

只保留诊断性的地址观察：时间表共 26,865 ms Running 权重。loomtty-server 的加载基址来自 trace frame binary；归一化到 Mach-O 0x100000000 后，按 ARM 4 字节边界向下对齐。Grid::scroll_up 的 0x1002ccc30、0x1002ccc2c 叶节点地址分别出现 3,255 / 1,044 ms 权重，对应重置新行前的环形行索引计算。样本地址不是逐指令退休周期，存在采样归因/调度扰动；不能据此宣称两条指令本身耗费相应比例。虽然汇编中存在 udiv/msub，直接落在它们地址上的叶样本为 0，不能将“除法慢”当成已证实根因。

原始 trace、time-profile XML 留在 /tmp/loom-scroll-pc-1003；不提交可能包含环境信息的原始 trace。归一化地址列表和 main 反汇编已保存。此次只据此提出可测试的快路径假设，没有宣称性能收益。

## 候选

基于 alacritty fork commit 867784628f619fc2f58c55d271ad5f6014d722c6，在独立 Git worktree /Users/linxy/.codex/worktrees/macos-native/loomtty/target/scroll-one-probe 创建本地候选 commit 1b584fcb（codex/scroll-one-probe）。没有修改 Cargo 的共享 checkout，也没有改变 Loom 主工作区依赖或用户安装的二进制。candidate.patch 可重新应用到该基线，不依赖本机 target 目录长期存在。

仅当 positions=1 且区域覆盖整个屏幕时走快路径：维持 display_offset、历史增长、淘汰计数原有语义，直接旋转环形存储并取得新底行进行 reset；区域/多行滚动走原始算法。减少通用循环、重复逻辑索引计算；取模也被单步边界更新取代，但这不是已证明的收益来源。

新增差分测试直接与保留下来的原始通用算法比较，覆盖 1/2/30 行视口，0/1/3/1001 行历史，每种组合 2,200 次写入/滚动，包含分区、多行操作、模板变化、滚回查看、回到底部、淘汰计数饱和，以及整个有效历史与视口逐行一致性。

137 项 alacritty_terminal 单元测试通过；对该包 all-targets clippy 通过。执行了 cargo fmt 后去除了无关文件的既有格式变更，最终候选只修改 3 个 grid 文件。

## 未完成的验证

已在独立 Loom worktree /Users/linxy/.codex/worktrees/extras-benchmark-control/loomtty（codex/server-scroll-probe，基线 d60c18a）临时使用本地 path 引用候选，Cargo.toml / Cargo.lock 的本机路径改动不用于合并发布。loom-term、loom-protocol、loom-app、loom-integration-tests 共 549 项通过、3 项忽略；该 worktree 的 cargo fmt --check、workspace all-targets clippy 通过。尚未做无干扰的真实 PTY/官方 vtebench 前后对照；不可合入 main 或宣传提速。下一步使用相同 main 客户端、分别连接 main/候选私有服务端，复测 scrolling、区域滚动、整屏 TUI 和内存。必须同时检查区域滚动退化，不能只保留命中的全屏单行场景。未经验证不更新 fork 远端或用户应用。
