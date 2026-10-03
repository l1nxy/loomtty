# 空 metadata 表的增量更新优化

实现：codex/client-delta-cpu，提交 dcace220，基于 main d60c18a，仅修改 ClientPaneGrid 的增量失效逻辑。OSC 8 超链接表与 grapheme 附加字符表为空时，跳过按覆盖格子逐个 HashMap::remove；非空时仍按原范围和历史偏移失效。不调整帧率、刷新量、分辨率、颜色、历史容量或辅助功能。

依据：基线 Time Profiler 的全屏场景里，仅 apply_delta_borrowed 内的 remove_entry 和 SipHash::write 就占客户端 3.09% Running 样本，另有循环自身耗时。完整栈见 ../macos-history-profile-2026-10-03。不是从代码外观猜测热点。

验证：cargo fmt、workspace all-targets clippy、392 项协议/App/跨 crate 集成测试通过（3 ignored），App Intents release 构建及 codesign strict verify 通过。已有测试覆盖普通/borrowed delta、跨行裁剪、历史偏移、组合字符和 OSC 8 覆盖失效。

无插桩三对 A/B：相同 main 服务端，100×30，Menlo 10，60 Hz × 1200 更新，顺序 before/after、after/before、before/after。6 次独立启动均完成，样本及哈希在 complete/results.json 和 validation.json。全屏总 CPU（一个核为 100%）三对为 13.114→12.791、14.500→13.859、14.574→13.897%；分别降低 2.46%、4.42%、4.64%，中位数 14.500→13.859%。局部 TUI 中位数 9.419→9.446%，首对回退 11.84%、其余 −1.22%/+0.29%，不能宣称局部收益。历史 footprint 132.38→132.25 MiB，视为持平。全屏 DSR p95 中位数 0.691→0.633 ms，p99 1.084→0.783 ms；这是服务端解析确认，不是显示或输入到像素延迟。

补充 profile：基线 full 的 apply_delta_borrowed inclusive 为 185 ms（8.66%），候选 full 有 1925 个 Running 样本，该函数为 37 ms（1.92%）；partial 1467 样本，该函数 3 ms。全屏该调用路径下没有再采到 HashMap 删除/哈希栈，机制与整程序 A/B 方向一致。profile 不计入无插桩 CPU 对比，也不直接把两次采样时间之差当精确性能收益。

启动故障隔离：最初基线启动停在 AppKit 的 NSPersistentUIRestorer / NSAlert::runModal，见 startup-failure/main-thread-sample.txt。先前 GL 探针退出后，复用的测试 bundle ID 触发恢复提示。后续将两侧测试包复制并改为各自独立 bundle ID，重新 ad-hoc 签名且 strict verify；嵌套服务端哈希仍与原始冻结服务端相同。isolated-binaries.json 保留改名前后的二进制哈希和 ID。失败启动不进入统计。没有改正式安装应用或用户偏好。

窗口按授权自动前台，没有连续遮挡监控。Cargo/Rustc 检查在阶段边界和启动间运行，不证明检查间隙完全无其他活动。三组全屏同方向下降，仍是当前机器/此测试场景的结果，不能扩大成所有 TUI 或所有终端负载。

该 CPU 改动已保留，并 cherry-pick 为 codex/client-cpu-memory 上的 84e257d；组合版本尚需验证，不把本次单项数据冒充组合数据。未合入 main、推送或安装。

组合验证中共享 Cargo target 曾解析到旧协议接口，编译明确失败，未产生候选包。组合构建改为 worktree 独立 target；没有把失败构建当通过。单项 dcace220 的原测试、Clippy 和 release 日志均确认重编译了该 worktree 的 loom-protocol 与 loom-app，且 profile 直接验证了新代码路径。
