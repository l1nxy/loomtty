# CPU 与历史内存组合候选的验证

候选为 codex/client-cpu-memory 的 84e257dc48f5cb69666f5b35621983af088acd95，基于 main d60c18a，包含 eef91f5 的统一属性历史行紧凑存储、dc4bc2d 的直接 viewport 展开，以及 84e257d 的空 metadata 表跳过删除循环。服务端始终使用冻结 main 二进制；版本与签名后的哈希见 binaries.json。

结论：内存下降再次复现，组合版 CPU 验收未通过。不能宣称组合版同时降低 CPU 与内存，也不能把 CPU 单项的 −4.4% 套用到组合版本。代码保留在候选分支，未合入 main、推送或安装。

## 自动前台三对测试

使用 Computer Use 按测试包专用 bundle ID 自动选择窗口，在每次启动以及全屏/局部 TUI 计时前 Raise 并点击标题，再验证 AX 终端焦点。没有要求用户确认。100×30 格，Menlo 10，60 Hz、1200 次更新，客户端与服务端合计 CPU，以一个核为 100%。六次成功运行的 frozen hashes、格子指标和 workload 字节数一致；18 个前置记录均匹配相应客户端 PID。没有连续监测遮挡，AX 焦点也不是整段采样持续前台的证明。

|配对|全屏 CPU 基线→候选|局部 CPU 基线→候选|历史 footprint MiB 基线→候选|
|---|---:|---:|---:|
|0|17.653→16.420|12.350→6.638|130.361→117.002|
|1|8.732→10.714|7.682→10.352|130.799→117.470|
|2|10.214→14.399|5.868→11.284|130.252→118.080|

历史 footprint 中位数 130.361→117.470 MiB，下降 12.891 MiB（9.89%），每对均下降。全屏 CPU 一组降低、两组升高，局部同样不一致；同一服务端的 CPU 也大幅变化。原因尚未证实，不能把差异全归因于窗口遮挡或全部归因于代码。DSR 分位数同样不稳定，原始结果全部保留；DSR 衡量解析确认，不代表输入到像素延迟。controlled/report.md 为脚本机械汇总，性能结论以本说明的限制为准。

本次存在两项执行异常：第一轮后控制器在第二轮启动的前置等待超时，退出码 1；恢复控制器复用已完成的两次独立运行，再执行余下四次，不能描述为不中断的三轮。其次旧 CUA binding 意外拉起了额外测试包客户端 PID 92936（00:17:47），在 r1-before 开始前清理；没有该额外进程的 CPU 计时，不据此宣称受影响样本无干扰。r0-before 启动前用 Raise+截图，其他启动用 Raise+标题点击；所有 TUI 前置动作相同。interrupted/ 保存失败批次，controlled/ 保留六次独立成功运行及验证；没有删掉不好看的 CPU 结果。

之前未逐阶段控制前台的组合探索批次在 exploratory-uncontrolled/，由于高波动和不对称界面检查，不用于性能验收。

## 构建与功能验证

组合候选使用 worktree 独立 target 完成 cargo fmt --all --check、cargo clippy --workspace --all-targets、App/协议/集成测试（397 passed，3 ignored）和带 macos-app-intents 的 release 构建，并通过 ad-hoc codesign strict verify。测试和构建日志在本目录。早先共享 target 解析到旧协议 API，编译明确失败；失败日志保留，未作为通过依据。没有在采样期间编译。

## 已成立的独立证据

CPU 单项 dcace220 在 ../macos-delta-empty-2026-10-03/ 的三个配对全屏 CPU 均下降，中位数 14.500→13.859%（−4.42%）；局部 TUI 无稳定收益。Time Profiler 证实 apply_delta_borrowed 的空表 HashMap 删除/哈希栈消失。该独立结果与本组合批次分开报告。

内存改动在 ../macos-compact-history-2026-10-03/ 的独立三对测试也下降约 9.6%，但有解码/展开 CPU 成本，并非 CPU 优化。四终端的旧比较位于 ../macos-main-four-complete-2026-10-03/，不能用不同时间的组合数值替换那张四终端表。
