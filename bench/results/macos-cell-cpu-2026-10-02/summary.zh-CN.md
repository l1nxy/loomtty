# macOS TUI CPU · 字符属性与文本快照 · 2026-10-02

**热点判定补充：** [Time Profiler 全线程活跃 CPU 排名](../macos-time-profiler-2026-10-02/summary.zh-CN.md)。本页的小幅 A/B 收益不证明这些小函数是主要瓶颈；后续以该排名定位。

[完整报告](report.md) · [原始数据](results.json) · [采样](profiles/) · [验证日志](validation/)

基线为上一轮 `d46eddc` 的 release + macos-app-intents。本轮重新采样客户端与服务端，保留同一服务端二进制，只优化客户端；三轮交错对照，共 6 次启动、120 个有效阶段。

## CPU 结果

单位是单核百分比；合计包含客户端与私有服务端。中位数后的括号为最小–最大值，不是置信区间。

| 场景 | 优化前 | 优化后 |
|---|---:|---:|
| 全屏 TUI / 合计 | 12.24 (12.13–12.52) | 11.82 (11.75–12.07) |
| 全屏 TUI / 客户端 | 8.01 | 7.63 |
| 全屏 TUI / 服务端 | 4.23 | 4.18 |
| 单行 TUI / 合计 | 6.76 (6.69–6.76) | 6.62 (6.55–6.99) |
| 单行 TUI / 客户端 | 5.23 | 5.15 |
| 单行 TUI / 服务端 | 1.51 | 1.47 |

全屏合计中位数下降 3.4%，客户端中位数下降约 4.7%；三组配对结果均下降（12.239→11.818、12.129→11.747、12.521→12.070）。单行有一组反向变化（6.756→6.993），不将其中位数的小幅下降视为稳定收益。各组件独立取中位数，不能保证相加等于总量中位数。服务端二进制完全相同，其波动不是代码优化成果。

上一轮同一个基线版本曾测得全屏 11.91%，这次重测是 12.24%。系统调度和后台负载会影响不同批次，收益只按本轮交替配对计算，不拼接跨批次数字。

暖机 RSS 115.34→115.27 MiB，物理占用 68.22→68.02 MiB，未出现明显内存增长。全屏 DSR p95 0.581→0.559 ms；单行 0.196→0.183 ms；全部 12 个 TUI 阶段解析确认超期为零。DSR 是服务端解析确认，不是 GPU 呈现、FPS 或按键到像素延迟。

## 改动

1. 每行重建时缓存连续相同的前景色、背景色与完整属性标志。相邻字符只解码字符本身；字体样式、宽字符、隐藏、反色、暗化、下划线等变化会重新计算。缓存借用本次颜色表，生命周期限于一行重建，不会跨主题沿用旧颜色。
2. 没有下划线或删除线的字符跳过装饰绘制函数；有装饰时继续原有路径。
3. 无障碍 UTF-16 文本快照按已知行宽和最终长度预留空间，减少反复扩容，并避免重复字符解码。行数/文本长度上限、密码隐藏、组合字符和选择范围逻辑不变。

没有改变帧率、渲染分辨率、GPU shader、原生功能开关或服务端行为。此前服务器采样中的 PTY 处理、termios 查询、行指纹及协议编码依然存在；本轮没有靠降低状态检查频率换取 CPU 下降。

## 采样和验证

基线全屏调用栈包含 CellProps::from_packed_cell_fast、ColorTable::resolve_packed、装饰处理；原生状态同步包含无障碍文本构建和分配。前后采样各使用独立 1200 帧运行，客户端与服务端分别 sample 10 秒，采样不混入正式统计。sample 包含等待栈，不将样本比例当作函数 CPU 百分比。

`cargo test -p loom-render -p loomtty --features loomtty/macos-app-intents`：479 项通过、1 项忽略。新增测试穷举所有 16 位标志，配合 named/indexed/RGB 前后台色和 ASCII/中文/emoji/NUL，验证缓存命中与属性切换均等价于原始解码。现有装饰、增量更新和无障碍密码隐藏/UTF-16/选择/历史上限测试通过。cargo fmt、cargo clippy --workspace --all-targets、release 构建完成；Clippy 仅有现有 warnings。

正式运行：Apple M6、macOS 27.0；100×30、Menlo 10pt、1× DPI，600×400 logical pixels；before/after → after/before → before/after。每个 TUI 阶段 600 帧、60 Hz、约 10 秒，bulk 各至少 8 MiB。CPU 包含 250 ms 尾段，不含 workload、shell、WindowServer 或 GPU 执行时间。前后网格、DPI、工作负载字节数/哈希和帧数核对一致。不能排除所有后台运行波动。

## 复跑

```sh
python3 bench/compare_loom.py --before /tmp/loom-cpu3-before \
  --loom target/release/loomtty --loom-server target/release/loomtty-server \
  --rounds 3 --frames 600 --loom-width 600 --loom-height 400 \
  --output /tmp/loom-cpu3-repeat
```
