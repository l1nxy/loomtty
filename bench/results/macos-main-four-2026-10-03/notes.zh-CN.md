# main 四终端复测：待空闲环境重跑

使用 main d60c18a 的 release、macos-app-intents 构建；签名后客户端/服务端哈希见 provenance.json。工作分支只提供已修复启动时钟的 runner，不能把它的 HEAD 误认作测试二进制来源。

第一批 Kitty 运行期间遇到其他任务并发 Rust 编译。整批 interrupted 不参与性能结论。第二批增加启动前连续 30 秒无 Rust 构建进程的检查，但新的编译持续出现，未完成任何终端的有效整套测试；保存于 waiting。

两批控制器均通过 SIGINT 进入 finally 清理，仅结束自己启动的窗口、工作进程和临时 caffeinate。没有停止其他任务的编译或用户终端。四终端三轮复测尚未完成，没有拼入昨天或未完成场景冒充最新结果。

用户明确要求自动前台，不再逐窗口确认。Loomtty 在内部文件门禁处由 agent CUA Raise + AX/截图核对；另外三款按用户确认的 CLI 启动自动前台行为运行，记录该假设，不伪称 CUA 验证。不是连续可见性监控。

后续使用 run-four.py 换一个新的空输出目录，在图形桌面解锁且无其他编译/性能测试时重新运行。已有 upstream checkout 位于 /Users/linxy/.codex/worktrees/macos-native/vtebench-extras-20261003，固定 commit ead80032e57dee2e75f0b51f2ea67528647d9944；已 release --locked 编译通过。不要安装未验证候选，当前实验用的是独立 bundle。
