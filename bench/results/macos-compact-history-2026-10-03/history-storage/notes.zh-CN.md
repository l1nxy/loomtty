# 历史存储微基准

这不是 GUI / GPU 测试，也不测用户可见帧率。`bench.rs` 是 dc4bc2d 内置的 `cargo build --release -p loom-app --example history_storage_bench` 示例；先构建，再直接执行 target/release/examples/history_storage_bench。采样期间每 250 ms 检查 Cargo/Rustc，发现即作废。结果记录每次操作的墙钟微秒，不是进程 CPU 百分比。

每种模式生成 10,000 行，分别 100 / 250 列。uniform 每格不同 ASCII 字符但相同属性；mixed 每格变换索引颜色，确保无法压缩。编码在测量前完成，两种解码逐格断言一致。每轮解码 20 次（包含释放），拼装 10,000 个 30 行视口，步长 17 行遍历历史；五轮交错顺序。dense 使用当前源码保留的原始 decode_sm_rows + Vec 行；compact 使用逐行回调和 ScrollbackCells。两者在同一二进制中测量，不能代替旧版/新版整进程 A/B。

`initial.csv` 对应 eef91f5 的逐行临时数组实现，`bench-initial.rs` 按测量时的逻辑保存；`direct.csv` 对应 dc4bc2d 直接拼装和移除完整行重复清零。不同时间批次存在频率/调度差异，不直接把两份 CSV 的绝对差当收益。summary.json 只比较 direct 批次内部交错的 dense / compact。

250 列时，整份 uniform 历史解码约 3.00 → 3.42 ms，每屏历史缓冲区拼装约 1.49 → 2.96 μs；mixed 整份解码约 10.93 → 11.75 ms，每屏约 1.77 → 2.18 μs。压缩带来的额外还原/判断成本确实存在，不能报告成 CPU 降低。GUI 活跃 viewport 保留连续密集表示；这里的额外成本只涉及历史接收/访问。每屏增加约 1.5 μs 尚小，但不代表超长选择、全历史辅助功能快照或持续重连没有代价。
