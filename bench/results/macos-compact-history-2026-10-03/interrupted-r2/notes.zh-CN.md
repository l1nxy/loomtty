本批采样期间助手误执行了 cargo fmt --check，因此整批作废；同时 runner 报告检测到 Cargo PID 16926。不能把这次中断归咎于用户的其他任务，也不纳入收益。后续采样期间不再运行 Cargo 命令。
