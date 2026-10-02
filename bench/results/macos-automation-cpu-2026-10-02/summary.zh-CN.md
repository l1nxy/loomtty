# macOS 自动化对象树 CPU 优化 · 2026-10-02

本轮只修改客户端的 AppleScript / App Intents 状态发布。服务端前后都是 111907d 的同一二进制，正式测试不包含 GPU 计数日志。

## 目标与实现

Time Profiler 的 GPU 调用链包含 drawable、render pass、驱动提交等，不能把整条调用链的百分比都算成上传成本。之前采样中 GPU 路径的内存复制只约占客户端 1%，本轮没有据此改 GPU 上传。

可明确消除的是每次终端文字更新都会重建两套自动化对象树：克隆标题/目录、生成对象 ID、创建 maps、发布快照。现在精确比较对象树的输入，只在窗口/原生 tab、连接、session、pane 顺序、焦点、标题或目录等变化时重建。没有使用可能碰撞的 hash，也没有延迟更新的定时缓存。

- AppKit 的 tab 顺序和选择仍每次观察，覆盖系统侧的拖动/合并。
- channel 身份及连接状态变化会失效，避免旧 terminal ID 操作新连接。
- 有等待中的创建命令时继续原路径，保留完成检查及超时。
- 只保存窗口编号和复制的标题，不持有 NSWindow；终端网格不被复制进缓存。

## 三轮正式 A/B

100×30、Menlo 10 pt、600×400、1×，TUI 每阶段 600 次更新、60 Hz、约 10 秒，CPU 含 250 ms 收尾。三轮交错顺序 before/after、after/before、before/after；6 次独立启动全部成功，前后工作负载字节、更新数和网格相同。CPU 单位是单核百分比，包含客户端和私有服务端，不含工作负载进程与 WindowServer。

| 全屏 TUI | before 中位数 | after 中位数 | 变化 |
|---|---:|---:|---:|
| 客户端 CPU | 7.983% | 7.522% | −5.8% |
| 服务端 CPU（同一二进制） | 3.566% | 3.521% | 小幅波动 |
| 合计 CPU | 11.549% | 11.028% | **−4.5%** |

三组全屏合计：11.434→11.021、11.752→11.028、11.549→11.338%，均下降。

单行合计中位数 6.585→6.433%，约下降 2.3%；但三组为 6.585→6.433、6.730→6.385、6.025→6.490%，最后一组反向变化，**不认定为稳定的单行收益**。不能用这次 Loomtty 数字与不同日期的其他终端数据拼接出精确排名。

全屏 DSR p95 中位数 0.477→0.466 ms；单行 0.187→0.191 ms；全部 TUI 解析确认超期为零。DSR 是解析确认，不是 GPU 呈现延迟。暖机 RSS 116.14→116.20 MiB，physical footprint 68.30→68.31 MiB；历史输出后 RSS 192.67→192.69 MiB，footprint 144.75→144.70 MiB。

## GPU 提交计数诊断

另用仅在 Metal submit 后增加一条计数日志的客户端，跑两轮交错对照。服务端仍为同一文件。这批 CPU 有日志开销，只用于诊断，不混入正式收益。

| 场景/组 | before→after 提交数 | before→after 客户端 CPU ms/提交 |
|---|---:|---:|
| 全屏/1 | 551→556 | 1.507→1.506 |
| 全屏/2 | 553→553 | 1.518→1.475 |
| 单行/1 | 551→551 | 1.023→1.006 |
| 单行/2 | 547→553 | 1.060→0.990 |

前后提交量接近，没有观察到优化后靠减少提交降低 CPU。全屏合计 CPU 这两组为 11.564→11.786%、11.650→11.428%，一升一降；诊断的整体收益不稳定，不把它写成额外的提速证据。600 次输出会被合并，提交次数不等于已呈现帧数，这批结果也不能反推未插桩正式测试的帧数。

计数代码已从源码移除。恢复后的重编译二进制 SHA 与正式被测版本不同，因此安装直接采用正式 A/B 留存的原始产物，核对 SHA 及无计数日志标记后重新打包；没有将带日志的诊断版本装入应用。详见 [frame-probe](frame-probe/)。

## 同服务端 Time Profiler 复采

前后分别录制全屏/单行各 20 秒；场景持续 40 秒，所有线程按 Running 权重统计。下表为 inclusive 权重，各行不可相加。

| 调用链 | 全屏 before→after | 单行 before→after |
|---|---:|---:|
| update_automation | 76→18 ms | 68→22 ms |
| Bridge::capture | 70→0 个采样 ms | 62→0 个采样 ms |
| update_native_state 总计 | 155→94 ms | 145→102 ms |
| 全进程 Running | 1557→1580 ms | 1131→997 ms |

优化后的 update_automation 采样来自元数据匹配。capture 未被采到不等于绝不会执行，只说明稳定文字输出不再反复构造对象树。全屏 profile 的全进程权重反而略升，因此不能用单次插桩总量宣称整体提速；收益以三轮未插桩 A/B 为准，profile 用于定位被消除的调用链。

余下大头仍是终端行/字形处理、服务端事件应用、GPU 绘制与 Metal 后台。优化后全屏采样中行/字形处理约 23.5%、GPU 绘制约 12.7%、已识别 Metal 后台约 17.3%；单行 GPU 绘制与 Metal 后台合计约 42%。本轮未解决这些成本。

原始 trace 含环境元数据，保留于 `/tmp/loom-native-cache-timeprofiler-{before,after}/`；仓库只保存不含环境变量的 Running 调用栈摘要。解析器为 [analyze.py](../macos-time-profiler-2026-10-02/analyze.py)，录制脚本及摘要在 [profiles](profiles/)。

## 验证与复跑

- `cargo test -p loomtty --features macos-app-intents`：389 passed、1 ignored。新增测试覆盖文字/光标/滚动的复用，以及标题、目录、连接、session、焦点、pane 顺序与移除的失效。
- `bash dist/macos/test-scripting.sh`：真实 Cocoa scripting contract 测试 1 passed。
- `cargo fmt`、`cargo clippy --workspace --all-targets`、release + `macos-app-intents` 构建通过；Clippy 只有现有 warnings。

[正式完整表](report.md) · [原始数据](results.json) · [验证日志](validation/)

```sh
python3 bench/compare_loom.py --before /tmp/loom-native-cache-before \
  --loom target/release/loomtty \
  --loom-server /tmp/loom-native-cache-before/loomtty-server \
  --rounds 3 --frames 600 --loom-width 600 --loom-height 400 \
  --output /tmp/loom-native-cache-repeat
```

正式客户端 SHA-256 before：`ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`，after：`bd09c5936127a337f3bcfe6bcc87231d16488b7a3d0d8a9a18e8d21fdade81dd`。服务端前后相同：`e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`。

## 本地安装

已用正式被测客户端打包 App Intents 版本并更新 `/Applications/loomtty.app`。codesign 严格校验通过；验证 45 个客户端、14 个服务端 Mach-O 代码/数据段与正式产物一致。只重启 GUI，34884→51800；服务端 PID 7961 及启动时间保持不变，`slim-bear` 的两个 pane 仍在，新 GUI 已连接。读取实际窗口的辅助功能树确认两个终端和上方 pane 按钮正常存在，没有向用户 pane 输入命令。

本轮客户端优化已生效。上一轮服务端优化仍需下一次服务端启动才能作用于用户现有 session，本轮没有为了更新而终止 PTY。旧应用已备份，详见 [installation.json](validation/installation.json)。
