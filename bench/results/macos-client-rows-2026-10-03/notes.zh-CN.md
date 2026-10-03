# 客户端行重建采样与被拒绝的属性借用候选

以冻结 main d60c18a 客户端、同一 main 服务端，各运行 40 秒全屏/局部 TUI，分别用 Time Profiler 采集 20 秒。没有将插桩运行当作未插桩 CPU 基准。采样分别包含 2053 / 1542 个 Running 样本，各有 2 个 sentinel 无栈样本，解析器明确计入无栈条目与总分母，没有静默删去。原始 trace/XML 保留 /tmp/loom-client-profile-1003，仓库只保存无环境信息的栈与记录脚本。

全屏客户端分类：行/字形处理 18.46%，GPU 前端 12.13%，已识别 Metal 后台 16.32%，应用服务端事件 9.74%，原生状态同步 7.99%。rebuild_row_render_data 自身 8.72%、precompute_row_shaping_into 自身 4.04%。父子 inclusive 百分比不相加。局部更新主要是 Metal 后台 23.22%、GPU 前端 16.15%、原生状态 12.71%，行/字形只有 2.46%。

候选将 CellPropsCache::get 从复制 CellProps 改为借用缓存，针对行重建里的颜色/样式复制。92 项渲染相关测试、14 项脚本测试、fmt、workspace all-targets clippy、App Intents release 构建均通过。构建/签名哈希见 binaries.json。

三对交错 A/B（before/after、after/before、before/after），同一服务端，100×30、1200 次/60 Hz 全屏和局部 TUI，6 次均完成。全屏 CPU 三对为 14.385→14.706、14.456→14.830、14.087→14.309%，全部变差，中位数 +2.23%。局部 CPU 中位数 9.581→9.417%，但方向不一致；历史 footprint 132.392→132.205 MiB，不支持有意义的内存收益。

候选已撤回，不合入 main，不替换安装应用。完整补丁与数据留存便于复核，不能将这次尝试写成 CPU 优化成果。下一候选在 codex/client-cpu-memory worktree 处理中，围绕实际历史行驻留内存；需单独验证。
