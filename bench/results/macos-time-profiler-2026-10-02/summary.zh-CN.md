# macOS TUI：按活跃 CPU 权重重新排序热点

这次纠正的是热点判定方法：先前 `sample` 调用栈能说明某条路径存在，但含等待栈，不能据此给全部 CPU 成本排序。上一轮全屏三轮配对降低 3.4% 是小幅实测收益，不足以证明已找到主要瓶颈。

本报告使用 Xcode **Time Profiler**，分别 attach 客户端与独立测试服务端。每个进程分别记录全屏/单行 TUI 20 秒，工作负载持续 40 秒（2400 次、60 Hz），避免录制启动时间挤占场景。100×30，Menlo 10pt，1× DPI，600×400 窗口。四次录制成功，私有测试实例已退出。未终止用户服务端或会话。

## 分母与限制

只汇总 `time-profile` 表中 `State=Running` 的采样权重；录制配置 `record-waiting-threads=0`，`time-sample` 的 `all-thread-states=NO`。分母是**被采进程所有线程的活跃采样权重**，不是主线程单独耗时，不是整个机器百分比，也不是 GPU 执行时间。系统调用栈即使带有 wait/park 名称，Running 采样仍保留；不把阻塞期间墙钟时长加进来。

每项 1 ms 权重：客户端全屏 1650 个、单行 979 个；服务端全屏 859 个、单行 302 个。四项分别录制，不能将客户端和服务端表的百分比直接相加。下面各表采用互斥分类，一个样本只归一次；JSON 另外保存 inclusive 函数排名，那些父子函数比例不可相加。

这是当前版本每个场景的一次统计采样，存在采样及 Instruments 自身开销，适合定位大项，不用于声称精确收益。局部场景服务端只有 302 个样本，较小函数的排名尤其不稳定。CPU A/B 仍应脱离 profiler、三轮交错测量。60 Hz 是输出更新频率，不是实测呈现帧率。

被测为 `d46eddc` 加上一轮尚未提交的字符属性缓存及 AX 文本分配改动，二进制指纹记录在 workload JSON。**这些是优化后的占比，不能反推出优化前函数的占比**。

## 客户端：全线程互斥分组

| 路径 | 全屏 TUI | 单行 TUI |
|---|---:|---:|
| GPU draw_frame 前台调用链 | 10.24% | 19.00% |
| 可识别的 Metal/驱动后台调用链 | 16.18% | 24.72% |
| 终端行构建、整形 | 22.61% | 2.55% |
| 应用服务器网格事件 | 12.55% | 1.84% |
| macOS 原生状态同步 | 9.70% | 15.22% |
| 其他场景构建 | 5.64% | 4.90% |
| 协议/IO 解码可归因栈 | 0.55% | 0.31% |
| 无法进一步归因的短 workqueue 栈 | 9.03% | 11.75% |
| 其他事件循环、运行时及 IO | 13.52% | 19.71% |

能归因的绘制及驱动合计 **26.42% / 43.72%**。后台分类依据 IOGPU、MTLCommand、AGX、FPCAMetalLayer、CAMetalLayer 栈；不会把只剩 `start_wqthread` 的短栈强行算作 Metal。其他事件/runtime 中还可能存在 drawable 生命周期工作，因此该归因并不覆盖所有绘图相关成本。

当前全屏 `Text::capture` 含子调用仅 **1.15%**，自身 **0.67%**；`CellProps::from_packed_cell_fast` 含子调用 **0.48%**。这些小函数已不是当前优先级。单行场景行构建仅 2.55%，继续优化整屏字符扫描并不能解决单行场景的主要开销。

代码与采样相符：`crates/loom-gpu/src/blade.rs::draw_frame` 每次获取 drawable、建立 render pass，上传背景/alpha/color 实例并提交。已有脏行缓存能减少 CPU 行重建，但没有令此后端只上传/提交那一行。这是后续应验证的路径；本采样**没有证明存在多余帧、也没有测量重复提交次数**。

## 服务端：全线程互斥分组

| 路径 | 全屏 TUI | 单行 TUI |
|---|---:|---:|
| 行指纹（包括字符打包） | 26.78% | 11.59% |
| 单元协议编码（包括字符打包） | 20.26% | 4.97% |
| PTY 输出处理 | 18.39% | 16.56% |
| 其他 damage 处理 | 6.29% | 4.64% |
| 其他 IO、tick、运行时 | 28.29% | 62.25% |

全屏指纹与编码合计 **47.04%** 服务端活跃采样。`crates/loom-term/src/pane/snapshot.rs` 中 `viewport_row_fingerprint` 调用 `pack_cell`，随后 `write_cells_into_sm` 又调用 `pack_cell`；服务端先筛选变更行再编码，存在重复打包路径。不能把整个 47% 当成可消除成本：其中还包含必须保留的哈希、状态编码、内存写入等。

相较之下，当前全屏 `__ioctl` 自身约 1.05%，没有理由优先降低密码/termios 状态检查频率。

## 优先级结论

1. **局部 TUI：绘制提交及 Metal 后台工作。** 下一步应测每次内容更新对应多少次提交，区分固定的每帧驱动成本、全量实例上传和真正多余的提交；不靠降低刷新率制造收益。
2. **全屏 TUI：服务端指纹/协议编码的重复打包，以及客户端行构建/整形。** 优化前后必须验证相同更新量、Unicode/颜色/属性、行同步正确性，再做未插桩 A/B。
3. AX 小额分配和字符属性零散操作不再作为主要优化方向。当前报告只重新定位瓶颈，没有新增实现改动或新的提速宣称。

## 可复核资料

- [全屏客户端](client-full.json)、[单行客户端](client-partial.json)、[全屏服务端](server-full.json)、[单行服务端](server-partial.json)：权重、互斥分组、inclusive/leaf 排名和去重栈。
- [客户端 workload](client-workload.json)、[服务端 workload](server-workload.json)：环境、二进制指纹、网格/工作量、阶段记录；这里带 profiler 的 CPU 不是正式 A/B 结果。
- `*-record.log`：四次录制完成日志。
- [record.py](record.py)：本机实际使用的控制器，保留当时绝对路径；复跑时调整路径并选择新的输出目录。
- [analyze.py](analyze.py)：解析 xctrace XML 的全局 id/ref，保留 Running 权重，生成没有环境变量的摘要。

原始 `.trace` 留在本机 `/tmp/loom-cpu-timeprofiler-{client,server}/r0-loomtty/`。原始 trace/TOC 包含进程环境变量，不纳入仓库。可导出并重新计算：

```sh
xcrun xctrace export \
  --input /tmp/loom-cpu-timeprofiler-client/r0-loomtty/tui_full-client.trace \
  --xpath '/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]' \
  --output /tmp/loom-client-full-repeat.xml
python3 analyze.py /tmp/loom-client-full-repeat.xml client
```
