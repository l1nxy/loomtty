# 同场景客户端 profile

main d60c18a 与内存候选 dc4bc2d，服务端同一冻结 main 二进制。100×30 Menlo 10，每个 GUI 各跑 40 秒全屏、40 秒局部 TUI，Time Profiler 各采样 20 秒。窗口 CLI 自动前台，没有持续遮挡验证。原始 trace/XML 在 /tmp/loom-history-profile-ab-1003；这里只保留不含环境变量的解析后栈。带 profiler 的运行不计入无插桩排名。

Running 样本：基线 full 2137、partial 1452；候选 full 2052、partial 1460。两版栈分布接近，未发现历史压缩函数在 TUI 中变成热点。单次 profile 不足以排除上一轮 CPU/延迟回退，也不能推断它已消失。

基线 full：行/字形 16.24%、GPU 前端 13.38%、已识别 Metal 后台 16.28%、服务端事件应用 9.73%、原生状态 8.80%。partial：行/字形 2.75%、GPU 前端 19.49%、Metal 后台 22.38%、原生状态 12.47%。候选 partial 的 GPU 前端 18.36%、Metal 后台 21.10%。分类优先级与重叠处理见 analyze.py；短 workqueue 栈单列，不强行归到 Metal。

下一步证据实验：同一 main GUI/服务端、只切换现有 blade / gl 后端，确认 GPU 提交路径与终端 CPU 差距的关系。该实验不修改用户设置或后端默认值。

OpenGL 探针失败：运行日志明确报告 macOS 禁用 GL，配置不支持，因此不作性能比较，不修改平台限制。失败阶段的计时/内存不能作为 OpenGL 结果。

追踪 full profile 中具体的哈希栈：RawTable::remove_entry 在 apply_delta_borrowed 下 37 ms（1.73% 客户端 Running 样本），SipHash::write 同路径 29 ms（1.36%）。读取代码发现每个覆盖格子都无条件向超链接和 grapheme 两张表执行 remove，即使表已空。创建 main 上的独立 codex/client-delta-cpu，给两张表分别加 is_empty 快速跳过；不改有附加信息时的失效语义，不携带历史压缩变更。等待无插桩 A/B，不从采样比例直接宣称整体 CPU 改善。
