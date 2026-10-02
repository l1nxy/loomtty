# 保留上游输出模式的补测（进行中）

上游 vtebench 不修改 termios；修正后的 runner 只在其 subprocess 执行期间恢复进入 raw 前保存的 PTY 输出标志，DSR 输入保持 raw，结束或异常时恢复设置。每阶段记录实际 output_mode。此目录不可与父目录 raw 吞吐量混合排名。

首个 Loomtty 运行完成全部 12 项，实际 output flags=3（OPOST、ONLCR）。没有更改被测二进制，scrolling 从 raw 的 28.2 变成 82.1 MiB/s、CPU 从 40.28 变成 16.70 ms/MiB。这是测试条件差异，不是终端优化收益。TUI/历史测试位于 vtebench 之前，执行方式未变。

另外三款尚待按相同输出模式补测；尚不能根据此目录宣称四款默认模式对照完成。

最近的 Kitty 补测在等待人工确认窗口可见时超时，未开始测量，不能计入结果。随后尝试 original 输出模式的 Loomtty scrolling Time Profiler 采样，Computer Use 返回 Mac 已锁屏；在可见性门槛之前取消了该轮，控制器已清理自己启动的客户端、服务端及 workload。没有新的有效采样，raw scrolling 的热点不能直接作为默认输出模式的优化依据。
