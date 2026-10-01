# macOS TUI 渲染 CPU 优化 · 第二轮 · 2026-10-02

[完整报告](report.md) · [原始数据](results.json) · [采样](profiles/) · [验证日志](validation/) · [代码补丁](implementation.patch)

基线为上一轮原生状态优化后的 main `51bf66e`。本轮继续优化字形缓存和屏幕坐标转换，三轮交错对照，共 6 次启动、120 个有效阶段。没有降低帧率或关闭功能。

## 实测结果

单位为单核 CPU 百分比，Loomtty 合计包含 GUI 和私有服务端；中位数，括号是最小–最大值，不是置信区间。

| 场景 | 优化前 | 优化后 | 相对变化 |
|---|---:|---:|---:|
| 全屏 TUI / 合计 | 12.92 (12.91–13.11) | 11.91 (11.82–12.61) | -7.8% |
| 全屏 TUI / 客户端 | 8.72 | 7.81 | -10.4% |
| 全屏 TUI / 服务端 | 4.27 | 4.10 | 未改动，属于运行波动 |
| 单行 TUI / 合计 | 6.99 (6.94–7.03) | 6.66 (6.58–6.96) | -4.7% |
| 单行 TUI / 客户端 | 5.50 | 5.18 | -5.7% |
| 单行 TUI / 服务端 | 1.49 | 1.47 | 未改动，属于运行波动 |

每一对运行的总 CPU 均下降；单行第一对差距只有 0.06 个百分点。各组件独立取中位数，不能保证相加等于总量中位数。服务端二进制完全相同，不把其波动归功于此次客户端优化。

暖机 RSS 114.78 → 115.33 MiB，约增加 0.55 MiB；暖机物理占用 67.67 → 68.22 MiB。缓存以少量内存换取重复计算减少。空闲 CPU 未出现持续忙循环。

全屏 DSR p95 0.568 → 0.553 ms，单行 0.185 → 0.188 ms；全部 TUI 阶段解析确认超期为零。DSR 只确认服务端解析，不代表 GPU 呈现、FPS 或按键到像素延迟。

## 改动和证据

- 字形 ID 查找增加固定 256 槽快速缓存，命中时直接比较完整 key，避免每个字符重复通用 HashMap 哈希。冲突回退原 HashMap；字体类别、字形完整 ID、粗斜体和宽字符仍参与区分。图集清空时同步清空两级缓存。
- 每行保留已转换的 GlyphInstance；内容变化时使该行失效，位置、缩放、焦点暗化或背景变化时重新转换。未变化行直接复制实例到提交缓冲区，避免逐字符重复 round、坐标和颜色计算。原有 GPU 上传、绘制、shader 和帧率策略保持原样。

基线采样的全屏行重建包含字形缓存哈希、属性解码和装饰处理，单行也包含整屏实例转换。新版单行采样不再出现逐字形转换的 RowInstances::get 热栈，但仍有实例复制、Metal 提交、原生状态及自动化快照开销。采样包含线程等待，只用于定位调用路径，不能用样本比例当作 CPU 百分比。

[字形缓存单独试跑](glyph-only-pilot/report.md) 只有一轮，未观察到明确整机收益；正式数据验证的是两项改动的组合，不能把全部收益归于其中一项。

脚本的单行模式轮换更新各行，约 0.5 秒后填满屏幕，后续每帧修改一行。它不是空屏上固定刷新唯一一行。完整帧则每次更新全屏。

## 尚未消除的差距

此次没有重新跑其他终端。此前四终端结果中 Alacritty 全屏约 8.5%，仍低于本轮 Loomtty 合计 11.91%；不同批次不能用来计算精确领先比例。新版全屏客户端中位数约 7.81%，私有服务端约 4.10%。后续减少整帧提交和服务端处理成本仍有必要，当前结果不能称为已经追平其他终端。

## 条件与验证

Apple M6、macOS 27.0；100×30、Menlo 10pt、1× DPI、6×12 单元格，窗口 600×400 logical pixels。前后顺序 before/after → after/before → before/after；TUI 各 600 帧、60 Hz、约 10 秒；bulk 各至少 8 MiB。CPU 含 250 ms 尾段，不含 worker、shell、WindowServer 和 GPU 执行时间。前后网格、DPI、负载字节数/哈希、帧数均核对一致；采样与正式运行分开，不能排除所有后台调度波动。

`cargo test -p loom-render -p loomtty --features loomtty/macos-app-intents`：478 项通过，1 项忽略。新增/扩展验证覆盖缓存冲突、样式/字体/宽字符区分、图集清空、透明字形、增量重建、滚动行复用、坐标/缩放/暗化/背景失效。cargo fmt、cargo clippy --workspace --all-targets 和 release 构建通过；Clippy 保留现有 warnings。

已安装到 `/Applications/loomtty.app`；签名验证通过，打包后的客户端 45 个、服务端 14 个 Mach-O 内容 section 与实测 release 一致（签名会改变整个文件哈希）。原服务端 PID 7961 和 slim-bear 的两个面板保留，GUI 为 PID 32672。界面检查及两面板焦点切换正常，已恢复原焦点；没有改动用户配置。旧应用备份在 `~/Library/Application Support/loomtty/install-backups/20261002-004841/loomtty-before-render-cpu.app`。

## 复跑

```sh
python3 bench/compare_loom.py --before /tmp/loom-tui-before-render-20261002 \
  --loom target/release/loomtty --loom-server target/release/loomtty-server \
  --rounds 3 --frames 600 --loom-width 600 --loom-height 400 \
  --output /tmp/loom-tui-render-compare-new
```
