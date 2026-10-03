# 断点续跑 g（中断）

严格校验哈希和参数后复用了 f 批次的完整 Kitty 第 0 轮；这里的 Kitty 不是一个新增独立样本，原始 .dat / manifest / ready 保存在相邻 auto-foreground-checkpoint-f/r0-kitty。聚合时按 source_directory 去重。

Alacritty 重新启动，完成全屏 TUI 后检测到并发 Rust 构建（60649），整个启动失败，不纳入中位数。该失败已进入顶层 results.json/runs 与报告，不仅存在日志。随后控制器在启动 Ghostty 前停止，所有私有测试进程已由 runner 清理。尚未得到三轮四终端结果。

继续时使用本目录对应原始 /tmp/loom-four-auto-1003g 作为 checkpoint，以及一个全新的输出目录。复用 Kitty，不复用失败的 Alacritty；保持自动前台和相同源代码/二进制/负载/参数核验。
