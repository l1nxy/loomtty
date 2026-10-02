# 官方 vtebench 接入与历史解码内存优化 · 2026-10-02

本轮完成官方程序接入、四终端启动/负载一致性检查、Loomtty 的 12 个官方默认场景，以及一次有分配栈依据的客户端内存优化。**四终端正式横向测试尚未完成**，不能把这里的 Loomtty 单次官方数据写成四终端排名。

## 官方来源与当前覆盖

使用未修改的 [alacritty/vtebench](https://github.com/alacritty/vtebench)，锁定 commit `ead80032e57dee2e75f0b51f2ea67528647d9944`。接入脚本已提交为 `e65a472`；方法见 [vtebench-macos.md](../../vtebench-macos.md)。

- Loomtty、Alacritty、Kitty、Ghostty 均通过真实 GUI 启动、100×30 网格检查，12 个默认场景的 setup/payload 字节及 SHA 全部相同。见 [四终端 probe](four-terminal-probe/)。Probe 不是性能结果。
- 上游生成器在 macOS 上遇到 BSD `ps` 的 tty 字段尾随空格，私有 PATH shim 只修剪该字段；官方生成器文件和 Rust 计时逻辑均未修改。生成结果在真实 PTY 内预先冻结，避免上游静默忽略生成失败，原始 `.dat` 样本保留。
- 每个官方场景使用至少 16 MiB／样本、1 次 warmup、3 秒采样，完整沿用上游 write-blocking 计时。另测终端进程 CPU、RSS、physical footprint；CPU ms/MiB 的分母含 warmup payload，分子含 setup、warmup 和收尾。
- 官方测试是 PTY 读取/写入阻塞吞吐，不是实际呈现 FPS。仍保留独立 DSR、定速 TUI 和历史内存场景。

前台确认时，CUA 分别明确拒绝操作 `net.kovidgoyal.kitty`、`org.alacritty`（“for safety reasons”）。没有改用其他途径控制这些窗口。Kitty 的确认超时，Alacritty 等待由控制器中止，测试进程清理，原始记录在 [visibility-aborted](visibility-aborted/)。已请求用户手动确认，未收到答复。没有把这些运行纳入有效性能数据。

Loomtty 通过独立测试 bundle 被 CUA 识别、Raise 并截图确认，完成 [12 场景基线](upstream-before/report.md)。基线单次吞吐包括 cursor_motion 324.2、dense_cells 321.4、medium_cells 241.2、scrolling 28.8 MiB/s；不同负载不能直接互相比速度。原始 [results.json](upstream-before/results.json) 保存资源量和版本。独立 bundle 仅修改标识/显示名称及重新签名；45 个客户端代码/数据段已与此前正式产物逐段比对。

三轮内存 A/B 完成后，准备优化后官方复测时，CUA 报告 Mac 已锁屏、无法自动解锁。该后续运行没有前台确认，也没有开始性能测量。优化后的官方场景复跑和四终端比较仍待继续；没有用已有基线替代它们。

## 分配诊断与改动

独立诊断进程启用 MallocStackLogging，并用 `heap`、`vmmap`、`malloc_history` 检查历史输出前后。**这些诊断进程的资源数字不混入下面的未插桩 A/B**。启用栈记录会增加 footprint，虚拟映射大小也不能当作物理内存。

基线诊断中，客户端实际历史行约 17.0 MiB（9971 个 1792-byte malloc 块），服务端行存储约 24.4 MiB（10,000 个 2560-byte 块）；调用栈分别指向客户端 `apply_full_sync` 和 alacritty grid `scroll_up`。两端 `vmmap` 还各有约 **15.2 MiB 的 Malloc Large (empty) 驻留脏页**。见 [诊断摘要及调用栈](allocations-before/)；完整 malloc_history 保留在 `/tmp/loom-history-allocation-20261002/`，不把数百 MB 的系统调用栈全部放入 Git。

客户端原先将全部历史解码为一个连续 `Vec<PackedCell>`，再复制成各行；大数组释放后，malloc 仍可能保留其物理页。改为直接解码到最终拥有的逐行缓冲区，再移动进历史队列，消除这份完整临时副本。

- 复用同一套 opcode 校验与状态机，连续视口路径和逐行历史路径使用静态分派，避免另写一套易偏离的解码器。
- 保留颜色、flags、跨行 repeat/长字符序列、短流默认补齐、wrap flag 和组合字符索引。
- 保留协议格式、历史容量和替换/追加/裁剪语义；没有通过减少历史行数降低内存。
- 服务端未改。它的历史打包临时数组仍是后续排查对象，本次不能声称已消除两端全部中间存储。

## 三轮未插桩 A/B

每组分别启动 before/after、after/before、before/after，共 6 次有效运行；相同的服务端文件 SHA，客户端代码/数据段与各自被测 release 一致。100×30、Menlo 10 pt、600×400、相同 DPI；bulk 字节/哈希相同，每个 TUI 600 次更新／60 Hz，history 写入 10,000 行，两端配置容量仍为 10,000 行。

每次开始前都用 CUA 对应 bundle 的 Raise、AX 和截图确认窗口。没有连续监测运行中遮挡/锁屏；后续官方复跑准备时观察到锁屏的限制保存在 [visibility-limit.json](validation/visibility-limit.json)，不能据开始时的确认推断整段实际呈现帧数。

| 指标（中位数，客户端＋服务端） | before | after |
|---|---:|---:|
| 历史输出后 physical footprint | 145.03 MiB | **131.03 MiB** |
| 历史输出后 RSS | 200.00 MiB | **185.63 MiB** |
| 暖机 physical footprint | 67.99 MiB | 69.16 MiB |
| 全屏 TUI CPU（单核百分比） | 9.872% | 9.882% |
| 单行 TUI CPU（单核百分比） | 7.000% | 6.851% |

历史后 physical footprint 三组为 **145.10→131.74、145.03→130.39、144.97→131.03 MiB**，均下降；中位数减少 **14.0 MiB／9.7%**。暖机没有体现内存收益，其上升/波动保留在表中，不能宣称所有场景都省内存。

全屏 CPU 三组 9.700→9.882、9.872→10.624、10.029→9.612%，方向不一致，中位数基本持平。单行也有反向一组，不将其小幅中位数下降当作已证实 CPU 提速。

全部 TUI 解析确认超期为零；全屏 DSR p95 中位数 0.457→0.412 ms，单行 0.235→0.241 ms。DSR 只证明解析完成，不证明客户端显示完成。

[完整表](paired/report.md) · [原始结果](paired/results.json) · [二进制验证](validation/binaries.json)。当前三轮与此前未逐次确认窗口的其他批次不拼接成排名。

## 验证与待办

- `cargo test -p loom-protocol -p loom-app -p loom-integration-tests`：**392 passed、3 ignored、0 failed**。新增跨行、长 Unicode/ASCII run、颜色/样式、默认补齐、截断/溢出和尺寸上界验证；现有历史追加/替换/裁剪/组合字符/reflow 测试通过。
- Python bench 测试 **12 passed**，包含官方结果静默跳过、零精度样本及损坏 DAT 的拒绝。
- `cargo fmt`、`cargo clippy --workspace --all-targets`、release + App Intents 构建通过；只有既有 Clippy warnings。
- 已生成 `target/macos-history-rows-20261002/loomtty.app`；正式 release 留存 `/tmp/loom-history-after/`。**本轮内存改动尚未替换用户安装应用**，待解锁后继续复测与安装。
- 尚需：优化后官方负载复测、再次检查大数组分配消失、四终端有效横向比较，以及根据新数据继续处理 CPU/服务端内存热点。完整目标仍未完成。

仓库中的诊断日志及 DAT 仅清理行尾空白，未修改数值或调用栈；原始文件仍在对应 `/tmp/loom-*` 目录。
