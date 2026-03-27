# Droid (Factory CLI) 架构逆向分析报告

## 1. 技术栈

| 项目 | 详情 |
|------|------|
| 运行时 | **Bun v1.3.10** (JSC 引擎, 非 V8) |
| 语言 | TypeScript/JavaScript, 单二进制打包 (147MB ELF, not stripped) |
| 依赖 | libc, libpthread, libdl, libm (纯动态链接, 无额外 native 依赖) |
| 日志 | Pino |
| 校验 | Zod (schema validation) |
| 加密 | AES-256-GCM (状态/凭据加密) |
| 监控 | Sentry (可选, FACTORY_ENABLE_SENTRY) |

## 2. 支持的模型

- **Anthropic**: claude-sonnet-4-5, claude-opus-4-5, claude-opus-4-6, claude-haiku-4-5
- **OpenAI**: gpt-5.1-codex-max, gpt-5.2, gpt-5.2-codex, gpt-5.3-codex, gpt-5.4
- **Google**: gemini-3-pro-preview, gemini-3-flash-preview
- **Xai**: grok-code-fast-1
- **Zhipu**: glm-4.6, glm-4.7
- **自定义**: 通过 settings.json 的 customModels 支持任意 OpenAI 兼容端点

## 3. 核心架构 — Mission 系统

### 3.1 整体流程

```
用户定义 Mission
    │
    ▼
Orchestrator (主 droid 进程)
    │
    ├── 解析 mission.md + features.json + validation-contract.md
    │
    ├── 按 Milestone 顺序执行
    │   │
    │   ├── Milestone 1: foundation-refactors
    │   │   ├── Feature A → spawn Worker Session → handoff
    │   │   ├── Feature B → spawn Worker Session → handoff
    │   │   ├── Scrutiny Validator → code review all features
    │   │   └── User Testing Validator → run assertions
    │   │
    │   ├── Milestone 2: ...
    │   └── ...
    │
    └── Mission Complete
```

### 3.2 Mission 目录结构

```
~/.factory/missions/{missionId}/
├── mission.md                 # 高层目标、计划、基础设施需求
├── AGENTS.md                  # Worker 共享上下文 (边界、模式、测试策略)
├── features.json              # Feature 数组, 每个是一个离散工作单元
├── validation-contract.md     # 验证断言定义 (VAL-{AREA}-{NNN})
├── state.json                 # 执行状态 (completed/pending 计数, worker session IDs)
├── validation-state.json      # 断言通过/失败追踪
├── progress_log.jsonl         # 审计日志 (事件流)
├── working_directory.txt      # Worker 执行目录
├── handoffs/                  # Worker 完成报告
│   └── {timestamp}__{featureId}__{sessionId}.json
└── evidence/                  # 验证证据 (截图、测试输出、终端日志)
```

### 3.3 Feature 定义

```json
{
  "id": "refactor-ciri-anim-crate",
  "description": "具体任务描述",
  "skillName": "要生成的 droid 类型",
  "preconditions": ["前置假设"],
  "expectedBehavior": ["成功标准"],
  "verificationSteps": ["验证命令"],
  "fulfills": ["VAL-FOUND-001"],
  "milestone": "foundation-refactors",
  "status": "pending | completed",
  "workerSessionIds": ["uuid-1"],
  "completedWorkerSessionId": "uuid"
}
```

### 3.4 Droid 配置 (Worker 类型)

位于 `~/.factory/droids/`, Markdown + YAML frontmatter 格式:

| Droid | 用途 |
|-------|------|
| **worker.md** | 通用 worker, 负责代码探索、研究、分析、实现 |
| **scrutiny-feature-reviewer.md** | 代码审查, 逐 feature 检查 diff |
| **user-testing-flow-validator.md** | 行为验证, 通过真实界面运行断言 |

### 3.5 Worker 生命周期

```
1. Orchestrator 选择 pending feature
2. Spawn Worker Session
   → 注入上下文: mission.md, AGENTS.md, feature details, validation-contract.md
   → Session 设置: model, reasoningEffort, autonomy
3. Worker 执行
   → 读代码、修改、运行测试
   → 独立 git 操作 (可能在 worktree 中)
4. Worker 完成 → 生成 Handoff
   → handoffs/{timestamp}__{featureId}__{sessionId}.json
   → 包含: salientSummary, whatWasImplemented, tests, discoveredIssues, skillFeedback
5. Progress logged → progress_log.jsonl
6. Orchestrator 继续下一个 feature 或进入验证阶段
```

### 3.6 验证两阶段

**Phase 1: Scrutiny (代码审查)**
- 为 milestone 内每个已完成 feature 生成 scrutiny-feature-reviewer
- 检查 diff vs feature 描述
- 发现 convention gaps, skill gaps, service gaps, knowledge gaps
- 综合发现更新 AGENTS.md (共享状态演化)

**Phase 2: User Testing (行为验证)**
- 将 validation-contract 中的断言映射到可测试行为
- 设置测试环境
- 通过真实 UI/CLI/API 运行断言
- 捕获证据 (截图、console 错误、网络请求)
- 标记断言: pass / fail / blocked / skipped

### 3.7 Validation Contract 格式

```markdown
### VAL-FOUND-001: Animation motion pipeline preserves easing semantics
- **Assertion**: cargo test -p ciri-anim passes, easing curves unchanged
- **Evidence**: test output screenshot
- **Pass criteria**: all tests green, no behavioral regression
```

## 4. Session 与持久化

### 4.1 Session 存储

```
~/.factory/sessions/{workingDirHash}/{sessionId}.settings.json
```

每个 session 包含: model, reasoningEffort, interactionMode, autonomy 设置

### 4.2 Snapshot 系统

```
~/.factory/snapshots/
├── manifests/{sessionId}.snapshots.json   # 文件变更边界、hash、时间戳
└── content/{hash}/                         # 按 content hash 去重存储
```

用于跨对话边界的文件变更追踪和恢复/重放。

### 4.3 History

`~/.factory/history.json` — 完整审计日志:
- message, mission_accepted, worker_started, worker_completed 等事件
- 时间戳、命令/消息、模式

## 5. Agent Browser 子系统

二进制中嵌入了一个完整的浏览器自动化引擎:

- **100+ action 类型**: launch, navigate, click, type, screenshot, fill, tab 管理...
- **加密状态管理**: AES-256-GCM 加密 session 文件
- **Auth Vault**: 加密存储登录凭据
- **Domain 过滤**: WebSocket/EventSource 域名白名单
- **Action Policy**: 基于策略的权限控制 + 危险操作确认流

## 6. 关键设计模式

### 6.1 Orchestrator 模式
- 中心化编排器分配 feature 给 worker
- Worker 通过 handoff 交还控制权
- 编排器协调验证 (scrutiny → user testing)
- 每个阶段后返回编排器

### 6.2 共享状态演化
- Worker 发现 convention/service/knowledge gaps
- Scrutiny 综合发现到 shared-state 改进
- AGENTS.md 记录这些发现供后续 worker 使用
- **核心洞察**: 系统会学习, 后续 worker 比前面的更有上下文

### 6.3 Evidence-Based Validation
- 所有断言必须有捕获的证据
- 证据按描述性文件名存储在 mission 的 evidence 目录
- 报告引用证据路径

### 6.4 Milestone 序列化
- Feature 按 milestone 分组
- 低风险 → 高风险 渐进执行
- 每个 milestone 完成后验证, 再进入下一个
- 前置 milestone 的发现反馈到后续 milestone

## 7. 实现细节深入

### 7.1 State Machine — Mission 状态管理

`state.json` 是 mission 的运行时状态机：

```json
{
  "missionId": "mis_35e2785d",
  "baseSessionId": "uuid",              // orchestrator 自身的 session
  "state": "running | completed",
  "workingDirectory": "/path/to/repo",
  "currentFeatureId": null,             // 当前正在执行的 feature (单个)
  "currentWorkerSessionId": null,       // 当前 worker 的 session ID
  "currentWorkerPid": null,             // worker 进程 PID (用于监控/kill)
  "workerSessionIds": ["uuid-1", ...],  // 所有 worker session 历史
  "completedFeatures": 41,
  "totalFeatures": 41,
  "orchestratorActedSinceReturn": false, // 防止 orchestrator 重复处理同一个 handoff
  "lastReviewedHandoffCount": 55,        // 游标: orchestrator 已处理到第几个 handoff
  "milestonesWithValidationPlanned": [   // 哪些 milestone 需要验证
    "foundation-refactors",
    "shared-contract-refactor",
    ...
  ]
}
```

**关键设计：**
- `currentFeatureId` 是互斥锁 — **同一时间只有一个 worker 在执行** (串行调度)
- `orchestratorActedSinceReturn` 防止 orchestrator 收到 handoff 后重复处理
- `lastReviewedHandoffCount` 是游标，orchestrator 只处理新增的 handoff

### 7.2 Worker 启动协议

Worker 被 spawn 后的**第一件事**不是写代码，而是执行标准化的 bootstrap 流程：

```
1. 调用 mission-worker-base skill    → 读 mission.md, AGENTS.md, validation-contract.md
2. 调用 feature-specific skill       → 读 feature 的 skillName 对应的 skill 文件
3. 创建 TODO 列表                    → 拆解 feature 为步骤
4. 读 mission 上下文文件              → mission.md, AGENTS.md, features.json
5. 读 repo 级别 AGENTS.md            → 可能有 repo 自己的规则
6. 读 .factory/services.yaml         → 知道有哪些可用的命令/服务
7. 执行 .factory/init.sh             → 项目初始化脚本
8. git log 看最近历史                 → 理解当前状态
9. jq 查询同 milestone 的其他 feature → 了解上下文
10. 读 .factory/library/ 下的知识文件  → 吸收积累的知识
```

然后才开始实际工作。这个 bootstrap 保证了每个 worker 都有**一致的起点**。

### 7.3 Worker 结束协议 — EndFeatureRun

Worker 完成后调用 `EndFeatureRun` 这个特殊 tool：

```json
{
  "successState": "success | failure",
  "returnToOrchestrator": true | false,
  "commitId": "git-commit-hash",
  "validatorsPassed": true,
  "handoff": {
    "salientSummary": "一句话总结做了什么",
    "whatWasImplemented": "详细描述",
    "whatWasLeftUndone": "未完成的部分",
    "verification": {
      "commandsRun": [
        { "command": "cargo test -p ciri-anim", "exitCode": 0, "observation": "..." }
      ],
      "interactiveChecks": []
    },
    "tests": {
      "added": [{ "file": "path", "cases": [{ "name": "...", "verifies": "..." }] }],
      "coverage": "覆盖说明"
    },
    "discoveredIssues": [
      { "severity": "blocking | non_blocking", "description": "..." }
    ],
    "skillFeedback": {
      "followedProcedure": true,
      "deviations": [],
      "suggestedChanges": []
    }
  }
}
```

**关键字段：**
- `returnToOrchestrator` — 如果 true，orchestrator 需要介入处理 (通常是 failure 或 validator)
- `skillFeedback` — worker 反馈 skill 文件是否好用，是否需要更新。这是知识回流的另一个通道
- `discoveredIssues` — 发现的问题，severity=blocking 会触发 orchestrator 创建 remediation feature

**CRITICAL**: Worker 调用 `EndFeatureRun` 后**必须立即结束**，不能继续工作。

### 7.4 Worker Transcript — 行为审计

每个 worker 执行完毕后，系统自动将其工具调用序列压缩为 **transcript skeleton**，写入 `worker-transcripts.jsonl`：

```json
{
  "workerSessionId": "uuid",
  "featureId": "refactor-ciri-anim-crate",
  "milestone": "foundation-refactors",
  "skeleton": "## Tool: Skill\n{mission-worker-base}\n## Tool: Read\n{mission.md}\n## Tool: Execute\n{cargo test}\n...",
  "timestamp": "ISO"
}
```

Skeleton 是 worker 完整对话的**压缩摘要** — 保留了工具调用序列但去掉了大部分内容。Scrutiny reviewer 用它来理解 worker "做了什么、怎么做的"，而不需要读完整对话。

### 7.5 Scrutiny Harness — 三级信息流

```
Level 1: Scrutiny Feature Reviewer (子 agent, 每个 feature 一个)
    │
    │ 产出:
    │   - codeReview: { issues: [...] }
    │   - sharedStateObservations: [
    │       { area: "conventions", observation: "...", evidence: "..." },
    │       { area: "skills", observation: "...", evidence: "..." },
    │       { area: "services", observation: "...", evidence: "..." },
    │       { area: "knowledge", observation: "...", evidence: "..." }
    │     ]
    │
    ▼
Level 2: Scrutiny Validator (父 agent, milestone 级别)
    │
    │ Triage — 三个桶:
    │
    │ ┌─ Apply Now (直接执行)
    │ │   适用: services.yaml, .factory/library/
    │ │   条件: 事实性、低风险、机械性
    │ │   → 直接修改文件 + commit
    │ │
    │ ├─ Recommend to Orchestrator (只建议)
    │ │   适用: AGENTS.md, .factory/skills/
    │ │   条件: 规范性决策, 需要 orchestrator 权限
    │ │   → 写入 synthesis.json 的 suggestedGuidanceUpdates[]
    │ │   → 包含: suggestion, evidence, isSystemic (是否跨 feature 复现)
    │ │
    │ └─ Reject (丢弃)
    │     条件: 重复、模糊、已有文档
    │     → 写入 synthesis.json 的 rejectedObservations[]
    │
    ▼
Level 3: Orchestrator (最终裁决)
    │
    │ 读取 synthesis.json:
    │ - blockingIssues → 创建 remediation feature, 重新分配给 worker
    │ - suggestedGuidanceUpdates → 决定是否更新 AGENTS.md / skills
    │ - 如果 status=fail → 创建 fix feature → 重新 scrutiny
    │
    ▼
AGENTS.md 被更新 → 下一个 worker/milestone 读取最新版本
```

### 7.6 Remediation 循环 — 失败恢复

当 scrutiny 发现 blocking issue 时的修复流程：

```
Scrutiny reports failure
    │
    ▼
Orchestrator 分析 blocking issues
    │
    ├── 创建 remediation feature (如 "fix-ciri-session-contract-regressions")
    │   ├── description: 从 scrutiny issue 自动生成
    │   ├── preconditions: 引用原始 feature 的 handoff
    │   └── fulfills: 复用原始 feature 的 VAL-* 断言
    │
    ├── Spawn fix worker → 执行修复 → handoff
    │
    ├── Re-run scrutiny (round++)
    │   ├── 只审查新增的 fix feature (不重复审查已通过的)
    │   └── fix reviewer 同时审查原 feature 和 fix feature
    │
    └── 循环直到 scrutiny pass → 进入 user testing
```

从实际数据看，ciri 的 foundation-refactors milestone 经历了 **5 轮 scrutiny** 才通过：
1. Round 1: 发现 session crate 3 个 regression → fail
2. Round 2: fix session → 但发现 input + layout regression → fail
3. Round 3: fix input/layout → 但 fmt check 失败 → fail
4. Round 4: fix fmt → 但 synthesis 逻辑没正确处理 fix-supersedes-original → fail
5. Round 5: 终于全部通过 → 进入 user testing

### 7.7 Validation Contract — 断言追踪

```markdown
### VAL-FOUND-001: Animation motion remains stable
The refactored `ciri-anim` crate must preserve critically damped motion behavior...
Evidence: `cargo test -p ciri-anim`; terminal output for spring and animation tests...
```

每个断言有固定 ID (VAL-{AREA}-{NNN})，被 feature 的 `fulfills` 字段引用。
`validation-state.json` 追踪每个断言的状态：

```json
{
  "VAL-FOUND-001": {
    "status": "passed",
    "validatedAtMilestone": "foundation-refactors",
    "source": "user-testing",       // 谁标记的 pass
    "updatedAt": "2026-03-24T..."
  }
}
```

### 7.8 Model 配置分层

```
settings.json
  └── sessionDefaultSettings.model     → 交互式会话的默认模型
  └── missionModelSettings
        ├── workerModel                → worker 用什么模型
        └── validationWorkerModel      → validator 用什么模型

mission/{id}/model-settings.json       → mission 级别覆盖 (运行时快照)
mission/{id}/runtime-custom-models.json → mission 启动时的自定义模型快照
```

Mission 创建时会快照当前模型配置，保证 mission 运行过程中模型不变。

### 7.9 并发控制

从 mission.md 看，droid 对并发有明确策略：
- **Code workers**: 2-3 个最大并发
- **Heavy validation (scrutiny/user-testing)**: **串行**
- 但从 state.json 的 `currentFeatureId` 设计看，**实际执行是严格串行的** (一次一个 worker)
- Scrutiny validator 内部的 feature reviewer 子 agent **是并行的** (通过 Task tool 并发 spawn)

### 7.10 Git 隔离策略

- Worker 只 `git add` 和 `commit` 自己 feature 涉及的文件
- 不 push，只本地 commit
- Worker 审查 `git status --short` 确认不误伤其他文件
- 脏 worktree 是正常状态 — 其他 mission 的未提交改动不影响当前 worker

## 8. 如果要复刻 Mission 系统

### 最小可行架构

```
你的 Agent CLI
├── Orchestrator (主进程)
│   ├── Mission Parser (解析 mission 定义)
│   ├── Feature Scheduler (按 milestone 调度)
│   ├── Worker Spawner (fork 子 agent 进程/session)
│   ├── Handoff Collector (收集 worker 产出)
│   └── Validator Coordinator (触发 scrutiny + testing)
│
├── Worker Agent (子进程)
│   ├── Context Injector (注入 mission 上下文)
│   ├── Tool Executor (代码读写、测试、git)
│   └── Handoff Generator (生成完成报告)
│
├── Scrutiny Agent (代码审查)
│   └── Diff Reviewer + Gap Analyzer
│
├── Testing Agent (行为验证)
│   └── Assertion Runner + Evidence Capturer
│
└── Persistence Layer
    ├── Mission State (JSON)
    ├── Progress Log (JSONL append-only)
    ├── Session Settings
    └── Snapshots (content-addressed)
```

### 核心要实现的能力

1. **Mission 定义格式** — mission.md + features.json + validation-contract.md
2. **Worker Session 隔离** — 每个 worker 独立 session, 可能用 git worktree
3. **Handoff 协议** — Worker 完成时输出结构化报告
4. **共享状态演化** — AGENTS.md 随着 mission 进展不断更新
5. **两阶段验证** — 代码审查 + 行为测试
6. **Progress 审计** — JSONL 事件流, 可追溯每一步

### 推荐技术选择

| 组件 | Droid 用的 | 你可以用 |
|------|-----------|---------|
| 运行时 | Bun | Rust (tokio) 或 Bun |
| LLM 调用 | 多 provider SDK | OpenAI 兼容 API |
| 进程隔离 | Session + worktree | git worktree + subprocess |
| 状态存储 | JSON/JSONL 文件 | 同上, 够用 |
| 浏览器自动化 | 内嵌 Playwright | 可选, 按需加 |
| Schema 校验 | Zod | serde + JSON Schema |
