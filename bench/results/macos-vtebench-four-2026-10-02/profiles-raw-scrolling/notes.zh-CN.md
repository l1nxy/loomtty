这是 raw 输出模式下官方 scrolling 的诊断采样，不是正式跑分。输入是 `y\n`，不是只有换行。20 秒 Time Profiler 中 server 的 Running 权重共 21,403 ms（多线程可超过 wall time）；Row::reset inclusive/leaf 51.60%，collect_scrollback_cells inclusive 13.27%，StateEncoder::push_cell inclusive 8.06%。这些百分比不能简单相加。

检查上游源码确认 vtebench 本身没有切换 termios，而集成 runner 此前继承 DSR 的 raw 输出模式，关闭 OPOST/ONLCR。这样 LF 不回到首列，滚动复用行时会清空更多已占用列。不能直接把此热点当作默认命令行 vtebench 的热点；先修复输出模式并复测，再决定优化方案。原始 trace/XML 保存在 /tmp/loom-vtebench-scroll-profile，仓库仅保存去环境信息、合并相同调用栈的 Running 摘要。

归档 record.py 是当时脚本，使用当时默认的 raw 输出模式。用当前 runner 重现时必须显式加 `--vtebench-output-mode raw`，并使用新的输出目录。
