# 断点续跑 h：等待空闲超时

控制器已终止（exit 1），在 180 秒内未获得连续 30 秒无 Cargo/Rustc 进程的时段。采样前曾实测 Cargo PID 68460；Alacritty 的启动目录尚未创建，本批没有新性能样本。Kitty 仅复用 f 的完整第 0 轮，不能算第二个样本。原始 .dat 在相邻 auto-foreground-checkpoint-f/r0-kitty。

同一外部干扰连续三轮目标工作中复现：第一轮 d/e 的 Kitty 被编译打断；第二轮 f/g 中 Kitty 完成，但 Alacritty 两次被编译打断；本轮 h 连启动前空闲门槛都未获得。已有六类 v3 滚屏 A/B、主线内存/CPU 优化证据保持有效，但完整四终端三轮对比未完成，不能宣称目标达成。

安全下一步依赖外部状态改变：其他构建暂停或结束，给基准足够空闲时段。没有停止无关进程或替换已安装终端。自动前台保持开启，不恢复人工逐窗确认。

恢复命令（在开发/results worktree 执行，输出必须是新目录）：

```sh
python3 target/extras-bench-20261003/resume-four-auto.py /tmp/loom-four-auto-1003h /tmp/loom-four-auto-1003i
```

控制器会核验设置、二进制、bench/upstream 哈希和机器标识，复用 Kitty，继续缺失的 Alacritty；失败和补测批次有独立来源记录。若 /tmp 已清除，须从仓库归档恢复元数据并保留 source_directory 映射，不能伪造新样本。
