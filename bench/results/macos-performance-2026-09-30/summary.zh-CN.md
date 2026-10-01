# loomtty 性能优化实测 · 2026-09-30

优化前后各 3 次独立启动、共 120 个有效阶段。以下为中位数；[完整表格及范围](report.md)、[原始数据](results.json) 和 [终端日志](logs/) 均已保存。

本次对照的客户端二进制完全相同，仅服务端构建不同。两者已包含此前的 macOS 字体/GPU 内存优化；本表衡量这次 PTY 调度与转义扫描优化的增量收益，不能当作相对最初版本的全部提升。

## 改动

- PTY 输出到达后立即处理；移除空 drain 后会阻塞新输出的固定 16 ms 休眠。查询回复不等待下一帧。
- 视口快照与发送按 `render.frame_interval_ms` 合并（默认 16 ms）。保留待发送 damage，并以定时器送出最后一帧；空闲不周期唤醒，DEC 2026 同步输出和光标提交仍受保护。
- OSC / DCS / Kitty APC / DEC CSI 侧协议扫描使用 `memchr` / `memmem` 跳过普通文本及无关 SGR，保留分块尾部、UTF-8 与终止符语义。

## PTY + 解析吞吐

每类至少 8 MiB，独立预热；单位 MiB/s，越高越好。

| 场景 | 优化前 | 优化后 | 变化 |
|---|---:|---:|---:|
| ascii | 156.8 | 218.6 | +39.4% |
| truecolor | 164.2 | 263.4 | +60.4% |
| cjk | 178.9 | 270.6 | +51.2% |
| emoji | 195.0 | 299.0 | +53.3% |
| scroll | 140.1 | 183.7 | +31.2% |
| cursor | 148.1 | 216.0 | +45.9% |

## 延迟、CPU 与内存

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| 空闲 DSR p95，ms | 13.629 | 0.137 |
| 负载 DSR p99，ms | 19.201 | 0.224 |
| 全屏 TUI DSR p95，ms | 17.582 | 0.746 |
| 全屏 TUI 响应超期 / 300 次 | 42 | 0 |
| 全屏 TUI CPU，单核 % | 16.2 | 14.2 |
| 单行 TUI CPU，单核 % | 5.1 | 6.5 |
| 预热后 RSS，MiB | 108.0 | 107.7 |
| 预热后 physical footprint，MiB | 57.1 | 57.1 |

单行 TUI 的 CPU 上升是本次可见的代价。它同时获得更及时的解析响应；不能把所有工作负载都描述为 CPU 优化。三次启动不足以估计长期分布，细小内存或启动时间差异不作为确定性收益。

## 环境与有效性

- Apple M6 / 32 GiB / macOS 27.0，交流电，Menlo 10 pt，100×30 网格。6 次有效运行均为 1× DPI、8×16 像素 cell、loomtty 窗口 800×512 logical pixels。
- before/after → after/before → before/after 交错顺序。一次 before 运行检测到其他工作目录的 Rust 构建而中止，保留为失败记录、排除统计；待空闲后按原顺序补足缺失运行。临时恢复控制器按已记录哈希校验二进制和 workload 脚本，只重跑缺失项，其源码一并留存。
- 所有有效运行均有完整 20 阶段；6 类 bulk 的字节数和 SHA-256 一致；无 panic、配置重载或 DPI 变化。并发编译检查在每阶段前后执行，不能排除采样间隔内或其他类型的系统负载。
- 旧四终端报告的 loomtty 有配置监听线程异常，且使用 2× DPI，因此旧数据不参与本次对照。新基线把配置放在独立安静目录，并在运行后审计日志。
- DSR 确认 PTY 输送与终端解析完成。loomtty 由服务端回复，它不测 GPU 呈现、FPS 或按键到像素延迟；TUI 超期也不是屏幕掉帧数。
- 资源包括 loomtty 客户端 + 私有服务端，CPU 以一个核为 100%，包含每阶段 250 ms 等待尾段；不含 Python workload、WindowServer 或 GPU 执行时间。RSS 可能重复计算共享页。

最新四终端横向结果见[独立比较报告](../macos-comparison-2026-09-30/summary.zh-CN.md)。

## 验证与复跑

- `cargo test --workspace`：1,608 passed、0 failed、12 ignored。
- `cargo clippy --workspace --all-targets`：通过，有现存 warnings。
- `cargo fmt --check`、`git diff --check`：通过。
- Python bench 测试：9 passed，覆盖 CPU 时钟单位、DSR 分片/超时/异常响应、UTF-8、分位数和日志有效性。
- 新增回归验证：通知中断等待且不丢后续许可、空闲无周期唤醒、配置的帧间隔、合并更新的最新 generation 与最终帧送达、协议扫描边界。

```sh
# 先保存待对照版本的 loomtty 与 loomtty-server，再修改并构建。
cargo build --release -p loomtty -p loomtty-server
python3 bench/compare_loom.py --before /path/to/before --rounds 3 \
  --loom-height 512 --output /tmp/loom-performance
```

源码在 `codex/new-branch`，基于合入 macOS 修复后的 `fe438ab`，包含未提交修改。确切二进制、脚本哈希、Git 状态和运行配置见原始 JSON。
