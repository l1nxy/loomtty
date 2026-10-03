# 历史行共享属性候选（待性能验证）

分支 codex/client-cpu-memory，独立 worktree /Users/linxy/.codex/worktrees/tui-client-cpu/loomtty，提交 eef91f5，基于 main d60c18a。未合入 main、推送或安装。

最新四终端测试中，Loomtty 历史后的客户端约 85 MiB、服务端约 47 MiB。此前分配诊断已确认客户端密集 PackedCell 历史行占约 17 MiB。本候选保留相同历史容量、行宽、颜色、所有 flags、原始字符字节、wrap 元数据及外置 grapheme 映射。属性完全一致且至少 8 格的历史行保存一个完整属性模板和每格 4 个字符字节；混合属性行仍密集存储。不能假定所有文本或行宽都可压缩。

协议新增逐行转换解码入口，共用现有 opcode 状态机与边界验证，只复用一个密集 scratch row，避免先建立整份密集历史再压缩的峰值。完整解码成功后才提交到客户端历史。损坏流丢弃结果；短流保留默认 padding。查看压缩历史时临时还原，live viewport 仍借用原有连续 cells。行访问的 Rust API 返回 Cow；CLI 与 wire protocol 不变。

验证：协议/数据模型/集成测试 396 passed、3 ignored；macOS App Intents 客户端测试 389 passed、1 ignored。新增与原解码器的差分检查覆盖跨行 run、属性、短流、截断和尺寸上界；压缩存储验证原始 UTF-8 字节、颜色/flags、混合属性回退、宽字符、滚动查看、选词和 reflow。cargo fmt、workspace all-targets clippy、App Intents release 构建与 codesign verify 通过，保留现有 warnings。

性能仍未验证。初次 A/B 在采样前检测到编译；r1 基线 TUI 被编译打断；r2 助手误在采样中执行 cargo fmt，已明确作废；r3 基线完整完成（history footprint 131.970 MiB），候选启动时检测到新的 Cargo/Rustc（17563、17564）。本次尚无一个完整候选样本，绝不从理论数组大小推断进程 footprint 或 CPU 收益。原始中断结果分别保存；无用户会话或无关构建被停止。

下一步：用户暂停其他编译/测试后，使用已冻结签名的二进制，在新目录运行 compare-compact-history-retry3.py 的三对交错 A/B（更换输出路径）。两侧使用同一个 main 服务端；100×30、1200 次/60 Hz TUI、10,000 行历史。检查 CPU、实际 footprint、DSR 延迟、滚动查看与辅助功能行为，再决定保留、修改或撤回。
