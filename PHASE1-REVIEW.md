# Phase 1 review findings — `feat/web-gateway`

两轮 harsh-review 输出的全部条目（针对 worktree 在 `/g/code/ciri-web`
的 3 个提交：`db97d2e`、`698d994`、`c5d2acb`）。

图例：

- ✓ 已修
- ⏳ 真问题，本分支待修
- ⚠ 边界 / 文档型，看你怎么取舍
- ❌ reviewer 自己撤回
- 🔗 不属于 Phase 1（dev 分支既有代码），记下来日后另起任务

---

## A. 与 Phase 1 直接相关

### A1. Token / 鉴权

| # | 状态 | 位置 | 问题 | 备注 |
|---|------|------|------|------|
| R1-6 | ✓ | `ws.rs:extract_token_query` | `?token=…` 没 percent-decode，含 `+`/`=`/`%`/非 ASCII 的 token 会静默认证失败 | 698d994：用 `percent_decode_str` + 三个新测试 |
| R1-11 | ✓ | `ws.rs:accept_ws` | token 走 URL 会泄漏到访问日志/浏览器历史/Referer | 698d994：增加 `Authorization: Bearer <token>` 路径（浏览器仍只能用 query） |
| R1-5 / R2-12 | ✓ | `ws.rs:constant_time_eq` + `mod.rs` | 长度不匹配走 fast path 会泄漏 token 长度；comment 误称 "constant time" | 698d994 改文档；c5d2acb 在 daemon 启动时强制 token ≥ 16 字节 |
| R1-19 | ✓ | `ws.rs:accept_ws` doc | "constant time" 的措辞误导 | 698d994 改文档 |
| R2-14 | ✓ | `ws.rs:extract_bearer` | `Bearer` 大小写敏感不合 RFC 7235；`BEARER`/混合大小写会回退 | c5d2acb 改成 `eq_ignore_ascii_case` |
| R1-10 | ⚠ | `daemon/mod.rs` ws_listener | 非 loopback bind 只 warn，不拒绝 | 当前设计：用户自己接 TLS。要不要硬性禁掉 `0.0.0.0` 默认配置可以另议 |
| R1-21 | ⚠ | `Cargo.toml` | `tokio-tungstenite` 没启用 TLS feature | 设计选择：服务端不做 TLS，上游反代终止。若以后想本地 TLS，得开 feature |

### A2. AsyncRead / AsyncWrite 适配器

| # | 状态 | 位置 | 问题 | 备注 |
|---|------|------|------|------|
| R1-4 / R2-26 | ✓ | `ws.rs:poll_write` | 错误信息说成 "flush buffer" 但是从 `poll_write` 抛 | 698d994 改文案 + 加 BufWriter 交互文档 |
| R1-13 | ✓ | `ws.rs:poll_read` | "drop the old buffer so the next Binary message can reuse the allocation" 注释错误（实际从未复用） | 698d994 改成 `self.read_buf = Vec::new()` + 注释如实说"释放，不复用" |
| R1-20 | ✓ | `ws.rs` / `ciri-protocol/codec/mod.rs` | 用了字面量 `16 * 1024 * 1024`，与 protocol 真常量无强约束 | 698d994 把 `MAX_DATA_FRAME_LEN` 提升为 `pub` 并加 `const _: () = assert!(…)` |
| R1-22 | ✓ | `Cargo.toml` workspace | `futures-util` features 不显式（`Stream`/`ready!` 靠传递依赖） | 698d994 改成 `["std", "sink", "async-await"]` |

### A3. 测试覆盖

| # | 状态 | 位置 | 问题 | 备注 |
|---|------|------|------|------|
| R1-14 | ✓ | `ws.rs::tests` | 缺 percent-encoded token 测试 | 698d994 加 3 个 |
| R1-15 | ⏳ | — | 没有真 WS handshake 集成测试（401、缺 token、上行 ws 配置） | 后续 Phase 2 联调时跟前端一起测，或单独写 `tokio::net::TcpListener` 的 e2e |
| R1-16 | ✓ | `ws.rs::tests` | `poll_read` 切小块的覆盖 | 698d994 `poll_read_drains_single_binary_message_across_short_reads` |
| R1-17 | ⏳ | — | `poll_flush` 内层 sink 返回 `Poll::Pending` 的反压路径无测试 | 用 `poll-once` 风格 + 一个能控制 backpressure 的 mock，较 fiddly |
| R1-18 | ✓ | `ws.rs:WsStream` | 测试无法构造 `WsStream<TcpStream>` 之外的实例 | 698d994 把 `WsStream` 泛型化（默认 `TcpStream`） |

---

## B. 不属于 Phase 1（pre-existing on `dev`），但 reviewer 顺手挖出来的

这些不是 web-gateway 引入的，是 dev 分支已有代码。等 Phase 1 合并后另起任务清理。

### B1. `ciri-plugin`（Lua VM）

| # | 位置 | 问题 |
|---|------|------|
| R2-1 / R2-25 | `ciri-plugin/src/events.rs:69-119` | `dispatch_void` 里 `disabled` 双重检查 + 误导性注释（"void" 暗示无错误跟踪，实际有） |
| R2-2 / R2-23 | `ciri-plugin/src/events.rs:104-119` | `dispatch_void` 失败计数器只增不减；`dispatch_first` 有 reset 但 `dispatch_void` 没有 → 偶发错误的 handler 会被永久禁用，且无测试 |
| R2-3 | `ciri-plugin/src/sandbox.rs:14` | `lua.set_hook(...)` 返回值被 `let _ = ` 丢弃；hook 没装上时，runaway Lua 脚本会挂死 server，没有兜底超时 |
| R2-4 / R2-22 | `ciri-plugin/src/lib.rs:182-203` | 测试通过 `engine.lua` 访问私有字段 — 外部插件作者无此能力，测试给的是假信心 |
| R2-5 | `ciri-plugin/src/events.rs:169` | `func.call::<Value>` 只接 Lua multi-return 的第一个值，错误的多返回静默吞掉 |
| R2-18 | `ciri-server/src/daemon/session.rs:513-531` | server 实际走 `ciri_session::agent::detect_agent`，Lua 的 `detect-agent` handler 从不被调用 — Lua plugin 系统当前在生产中是死的 |
| R2-19 | `ciri-plugin` | `PluginEngine` 是 `!Send`，与未来嵌入 `Arc<Mutex<Server>>` 不兼容 |
| R2-20 | `ciri-plugin/src/lib.rs:111-130` | `format_status_bar` 的 `pane_count: usize` 经 mlua 转换可能截断（极端值，不实际） |
| R2-21 | `ciri-plugin/src/lib.rs:220-227` | `infinite_loop_is_killed` 的断言是 tautology（`is_some() || is_none()`），不验证任何东西 |
| R2-28 | `ciri-plugin/Cargo.toml:7` | `mlua` 不走 workspace dep |
| R2-29 | `ciri-plugin/Cargo.toml:9` | `dirs` 单独声明而非 `workspace = true` |
| R2-30 | `.github/workflows/*` | `mlua` 的 `vendored` feature 编译需 C 工具链，CI/INSTALL 未明示 |

### B2. server 内部

| # | 位置 | 问题 |
|---|------|------|
| R2-7 | `daemon/server/mod.rs:522-545` vs `:103-129` | `apply_responses` 与 `dispatch_responses` 是两份近似拷贝；对 `ShutdownServer` 处理不一致（一份 panic、一份 ignore） |
| R2-8 | `daemon/server/session_mgmt.rs:159-177` | `handle_kill_session` 对不存在的 session 也返回 `SessionKilled`，客户端无法区分 |
| R2-9 | `daemon/server/template.rs:180-186` | `handle_apply_template` 给被踢的客户端发 `ServerShutdown` 但不从 `clients` 移除；若 `try_send` 失败，僵尸客户端残留 |
| R2-10 | `daemon/session.rs:683-710` | `build_state_sync` 里 `pane_ids` 与 `panes` 不一致时 silently skip，无日志 |
| R2-11 | `tray/menu.rs:137-149` | `sessions_hash` 是 O(n²) 查找；tray 5 秒一次 |
| R2-13 | `daemon/mod.rs:268-278` | `remote.enabled` 的 TCP 监听完全无鉴权，注释自称"SSH tunnel only" 但不强制 — 易误配 |
| R2-15 | `daemon/server/restore.rs:110, 135` | 硬编码 `(8.0, 16.0)` cell size，未来默认值若改会漏改 |
| R2-16 | `daemon/server/restore.rs:109` | `tile_h = vh / tiles.len()` 忽略 `SavedTile.weight`，初次创建时可能给出极小 PTY，客户端 attach 后才被纠正 |
| R2-17 | `daemon/session.rs:252-262` | `pane_grid_size_with_cells` 的 OOM 保护循环对极端 `MAX_GRID_CELLS=0` 不收敛（病态值，不实际） |
| R2-24 | `daemon/server/tests.rs:12` | `test_client` 的 mpsc rx 立即 drop，`try_send` 必失败 — 既有测试可能在掩盖发送路径错误 |
| R2-27 | `ciri-server/src/session.rs` | 文件只剩一句"Currently unused but kept for future CLI commands"；要么删要么填 |

---

## C. reviewer 自我撤回（不用管）

R1-1、R1-2、R1-3、R1-8、R1-9、R2-6（reviewer 一开始挑出来，分析后自己说不是 bug）。

---

## 建议下一步

1. **Phase 1 收尾**：处理 A3 里两个 ⏳（R1-15 集成测试、R1-17 backpressure mock），就可以打住进 Phase 2。两个测试都不阻塞功能，可以延后到 Phase 2 联调时一起补。
2. **B 全部转出 Phase 1**：开两个独立分支 / issue —
   `chore/plugin-events-cleanup`（B1）
   `chore/server-state-cleanup`（B2）
   原因：scope 漂移会让 web-gateway PR 评审困难，且这些都是 dev 期间累积的旧账。
