# 服务端字符打包与行指纹 CPU 优化 · 2026-10-02

本轮针对 [Time Profiler 确认的服务端热点](../macos-time-profiler-2026-10-02/summary.zh-CN.md)：全屏 TUI 的行指纹、单元协议编码合计占服务端活跃采样约 47%。客户端二进制完全相同，仅替换服务端。

## 实现

- 行指纹生成时保存打包后的 `PackedCell`，之后协议编码及多个客户端可以复用，避免同一批输出重复 `pack_cell`。
- 哈希连续的已打包行，替代逐格混合小块哈希；只对非空组合字符溢出另行计入列位置和长度，保留组合标记/ZWJ 的失效语义。
- 每批 PTY 输出前清空缓存的逻辑长度，保留容量；resize 释放旧行缓存。没有本批缓存的积累 damage 仍从实时网格编码，不发送旧数据。
- 缓存按视口行数有界，100×30 的字符数据约 47 KiB／pane，另有行 Vec 元数据。未改协议、刷新率、分辨率、密码状态检查和 GPU shader。

## 三轮正式 A/B

三轮交错顺序 before/after、after/before、before/after，共 6 次独立启动、120 个阶段。每项 TUI 600 次更新、60 Hz、约 10 秒；CPU 包含 250 ms 收尾；单位为单核百分比。前后使用同一个客户端文件和相同场景字节数、网格、DPI。三轮不足以给出精确总体置信区间。

| 全屏 TUI | before 中位数 | after 中位数 | 变化 |
|---|---:|---:|---:|
| 服务端 CPU | 4.304% | 3.542% | **−17.7%** |
| 客户端 CPU | 7.955% | 7.939% | 约持平 |
| 客户端＋服务端 CPU | 12.258% | 11.482% | **−6.3%** |

三组全屏合计：11.602→11.438、12.543→11.759、12.258→11.482，均下降。服务端分别 4.072→3.523、4.393→3.628、4.304→3.542。

全屏 DSR p95 中位数 0.560→0.484 ms；单行 0.198→0.198 ms。全部 TUI 解析确认超期为零。DSR 是服务端解析确认，不能代表实际 GPU 呈现或输入到像素延迟。

暖机合计 RSS 115.94→116.16 MiB，physical footprint 67.97→68.31 MiB；历史输出后 RSS 192.53→192.72 MiB、footprint 144.58→144.77 MiB。进程级内存包含分配器和图形运行时波动，不能把变化全部当作行缓存大小。

### 单行异常与复查（不隐藏不利结果）

第一批单行合计中位数 **6.051→6.796%（上升约 12.3%）**，三组均上升。服务端 1.515→1.456%，变化主要来自二进制相同的客户端。不能仅凭客户端未改就认定这是噪声，也不能宣称单行提速。

为区分提交次数与每次提交成本，使用同一个临时计数客户端进行两组交错诊断。仅在 Metal submit 后增加一次 info 日志，记录每个工作阶段的提交数，不用于正式 CPU 收益：

| 单行诊断 | before 提交数 | after 提交数 | before 客户端 CPU ms／提交 | after 客户端 CPU ms／提交 |
|---|---:|---:|---:|---:|
| 第 1 组 | 544 | 555 | 1.039 | 1.045 |
| 第 2 组 | 552 | 555 | 1.050 | 1.014 |

对应合计 CPU：6.99→7.11%、7.16→6.91%，一升一降，没有重现固定方向的回退。600 次输出更新并不等于 600 次 GPU 提交；提交也不等于显示器已呈现。以上只能说明提交量与成本在波动，**不能反推第一批没有计数的运行，更不能证明已解释第一批全部差异**。

临时计数代码已恢复，正式客户端 SHA-256 与基线重新核对完全相同。另做 1200 次更新／20 秒、两轮交错的未插桩复测，独立记录在 [confirm](confirm/)。几批结果不混合挑选，不将单行结果当作已确认收益；其主要绘制/Metal 成本仍未解决。

更长的未插桩复测，全屏两组为 **12.289→11.653%、12.716→11.731%**（约下降 5.2%、7.7%）；单行两组 **6.748→6.626%、6.872→6.675%**（约下降 1.8%、2.9%），均无解析确认超期。没有重现第一批的单行固定回退，但跨批差异仍大于该小幅收益，因此仍不宣称稳定的单行提速。全屏五组配对均下降。

## 优化后 Time Profiler

与先前使用相同的 40 秒场景、每阶段录制 20 秒，按 Running 权重统计服务端全部线程：

| 全屏服务端调用链 | before 活跃权重 | after 活跃权重 |
|---|---:|---:|
| 行指纹（含字符打包） | 230 ms | 126 ms |
| 单元编码（含字符打包） | 174 ms | 119 ms |
| 全进程 | 859 ms | 637 ms |

采样支持目标调用链成本缩小，但两个录制不是同期配对，不能将这些权重降幅替代未插桩 A/B。编码下仍有 `pack_cell` 样本，积累 damage 的安全回退没有被删掉。原始 trace 含环境元数据，留在 `/tmp/loom-pack-timeprofiler-server/`，仓库只留无环境变量的解析摘要。

## 验证

`cargo test -p loom-term -p loomtty-server -p loom-protocol -p loom-integration-tests`：**384 passed、3 ignored、0 failed**。新增测试比较缓存及实时网格的编码字节，覆盖 ASCII/CJK/emoji、truecolor、样式、组合标记、滚屏、备用屏幕、DEC 2026、多客户端复用及 resize/reflow；另验证相同 PackedCell 的组合标记换列会改变指纹。现有两端边界、后到组合字符及服务端同步测试也通过。

`cargo fmt`、`cargo clippy --workspace --all-targets`、release + `macos-app-intents` 构建通过；Clippy 只有现有 warnings。临时 GPU 日志未进入正式源码和 release 文件。

## 本地安装

App Intents 版应用已打包并更新 `/Applications/loomtty.app`，codesign 严格校验通过；已验证客户端 45 个、服务端 14 个 Mach-O 数据/代码段与被测 release 一致。替换前后保留 GUI PID 34884、服务端 PID 7961 及其启动时间，没有终止现有 PTY。运行中的服务端仍为旧进程，**本次服务端优化在下一次服务端启动后生效**，只重新开一个客户端不保证服务端重启。旧应用保存在安装备份目录，详见 [安装记录](validation/installation.json)。

## 复核与复跑

[正式完整表](report.md) · [正式原始数据](results.json) · [热点复采](profiles/) · [帧计数诊断](frame-probe/) · [更长负载复测](confirm/) · [验证日志](validation/)

```sh
python3 bench/compare_loom.py --before /tmp/loom-pack-before \
  --loom /tmp/loom-pack-before/loomtty --loom-server target/release/loomtty-server \
  --rounds 3 --frames 600 --loom-width 600 --loom-height 400 \
  --output /tmp/loom-pack-repeat
```

正式客户端 SHA-256：`ea535e57080500872ca7d3d9629dc199c40e97f1f0a2c84608d1b0ebc844b01a`；服务端 before：`5ed8af256617f9ddc789453a2444ca908efc71efea88f7b0c11a8e4d06b811df`，after：`e70f23856e71ed6c0ed764d91564d1aaf18f65ebd892cc02d180297ae9914188`。
