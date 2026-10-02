# 服务端历史快照：内存候选优化，尚未合并

这不是四终端性能排名，也不是官方 vtebench 结果。桌面仍被 macOS 锁住，无法完成前台可见性确认。这里仅验证服务端历史快照的一项候选实现。

先前 MallocStackLogging 记录显示服务端历史打包生成约 15.3 MiB 的大数组，释放后分配器仍保留约 15.2 MiB dirty 页面。本次让历史网格直接进入 StateEncoder，保留旧的 owned snapshot 接口用于对照。客户端协议、历史容量和内容未改变。接入覆盖首次连接、会话切换、模板恢复和 tick 增量历史发送。

## 三轮隔离诊断

100×30 网格，10,000 行历史；plain、colors、unicode 三种确定性输入。每种新开进程，顺序逐轮反转，旧/新各三次启动。私有 PTY 提供输入，标题完成标记之后再测量。每进程构造和编码 31 次完整快照；第一轮是冷样本，表中时间是剩余 30 次的中位数，再取三进程中位数。资源使用 libproc 读取进程本身的 physical footprint，不含 shell 子进程。

| 输入 | 发送后 footprint（MiB，旧→新） | 构造快照（ms，旧→新） | 快照+编码（ms，旧→新） |
|---|---:|---:|---:|
| plain | 47.47 → 31.48 | 2.67 → 4.76 | 5.15 → 4.86 |
| colors | 46.64 → 30.59 | 2.66 → 4.48 | 5.02 → 4.65 |
| unicode | 51.53 → 35.92 | 2.98 → 5.42 | 5.75 → 5.64 |

消除了 16,000,000 字节的 PackedCell 历史数组；相应编码流分别为 550,822 / 380,822 / 810,822 字节。表中的约 16 MiB 是 headless 诊断进程的下降，不是已安装应用或正式 daemon 的实测收益。allocator 缓存、重复快照会影响绝对 footprint。

计时排除了 checkpoint 等待和对象析构，原始 JSON 同时保留 libproc 累计 CPU，用于避免把 wall-clock 微小变化误判成整体 CPU 改善。Unicode 的 CPU 未显示稳定改善。本轮不声称全屏 TUI CPU 降低。

## 为什么暂不合并

tick 原来在锁内复制历史、锁外编码历史；新路径在锁内编码历史。构造快照的时间因此增加约 2 ms，虽然总快照+编码时间略降，但不能据此排除多客户端输入延迟回退。需要补并发输入延迟和 GUI 正式对照后，再决定保留、限制使用范围或撤回此方案。当前仅提交到开发分支，不替换安装包、不重启用户 daemon、不推进 main。

GUI 工具再次返回 Mac locked；此前 Kitty / Alacritty 控制也被工具拒绝，后续对照仍需用户解锁并协助前置对应测试窗口。未绕过系统锁屏或工具限制。

## 正确性与构建

- 676 项测试通过，3 项已有 ignored；覆盖 protocol、term、app、server 和 integration。
- 新增 wire identity 测试：空历史、饱和历史、部分/越界 watermark、组合字符、宽字符、样式、标题/cwd、超链接、resize 和备用屏幕切换，新旧压缩帧逐字节相同。
- workspace clippy、fmt、macOS App Intents release 构建通过；clippy 保留既有警告。
- 新增控制脚本的超时检查后，另跑六进程 smoke 验证，不混入以上三轮数据。
- 原始数据和验证日志随报告保存。测试进程已退出；用户现有 GUI、服务端和 PTY 未被操作。

## 复现

```sh
cargo build --release -p loom-term --example history_snapshot_bench
python3 bench/history_snapshot_bench.py --output /tmp/history-snapshot --rounds 3
```

运行期间不要同时编译或进行其他性能测试。该脚本仅适用于 macOS，资源采样依赖 libproc；Rust example 不需要 GUI。比较的是同一编译产物的旧 snapshot 路径和候选 prepared 路径。
