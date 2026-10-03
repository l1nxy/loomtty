# main 四终端复测：待空闲环境重跑

使用 main d60c18a 的 release、macos-app-intents 构建；签名后客户端/服务端哈希见 provenance.json。工作分支只提供已修复启动时钟的 runner，不能把它的 HEAD 误认作测试二进制来源。

第一批 Kitty 运行期间遇到其他任务并发 Rust 编译。整批 interrupted 不参与性能结论。第二批增加启动前连续 30 秒无 Rust 构建进程的检查。等待后编译结束，Kitty 已开始采样；agent 依据之前的等待状态误判并主动中断了这次运行。此中断不应归因于持续并发编译；该部分数据未完成整套测试，仍全部排除，保存于 waiting。

两批控制器均通过 SIGINT 进入 finally 清理，仅结束自己启动的窗口、工作进程和临时 caffeinate。没有停止其他任务的编译或用户终端。四终端三轮复测尚未完成，没有拼入昨天或未完成场景冒充最新结果。

用户明确要求自动前台，不再逐窗口确认。Loomtty 在内部文件门禁处由 agent CUA Raise + AX/截图核对；另外三款按用户确认的 CLI 启动自动前台行为运行，记录该假设，不伪称 CUA 验证。不是连续可见性监控。

后续使用 run-four.py 换一个新的空输出目录，在图形桌面解锁且无其他编译/性能测试时重新运行。已有 upstream checkout 位于 /Users/linxy/.codex/worktrees/macos-native/vtebench-extras-20261003，固定 commit ead80032e57dee2e75f0b51f2ea67528647d9944；已 release --locked 编译通过。不要安装未验证候选，当前实验用的是独立 bundle。

第三批（repeated-interference）等待编译结束后分别启动 Kitty、Alacritty，两个运行都再次被采样检查捕获到并发 Rust 构建，已判失败。随后停止该批次并清理其拥有的进程。四终端复测仍未完成，需要实际连续空闲的机器；不是缺少前台确认或用户授权。

后续 goal turn 再次检查到无编译且桌面可用，启动按轮保存的 round0；Kitty 在第三项上游测量前再次被并发 Rust 构建打断，仍未得到有效完整一轮。日志保存于 continuation-round0。与此同时完成了独立的单行滚动候选与差分测试，详见 ../macos-scroll-one-probe-2026-10-03/notes.zh-CN.md；这不替代缺失的四终端正式对比。
