# EVMEventLake AI Agent Guidelines

欢迎来到 EVMEventLake 代码仓库。所有在此仓库工作的工程开发代理（AI Agent）必须严格遵循本规范。

---

## 1. 核心指令与规范原则

仓库的顶层工程开发规范已全面固化在：
👉 [**`docs/AI_AGENT_PROMPT.md`**](file:///ssd0/git/EVMEventLake/docs/AI_AGENT_PROMPT.md)

**核心原则速览**：
1. **一次只处理一个 Goal 和一个当前 Task**。
2. **一个 Session 默认最多完成一个 Task**。
3. **不要猜测，不要扩大范围，不要一次实现多个任务**。
4. **所有结论必须基于实际读取或实际运行的结果，没有运行过的测试不得声称通过**。
5. **在修改代码前必须按标准格式输出执行计划（Request Type, Goal, Files To Modify, Acceptance Criteria 等）**。
6. **每个 Session 结束前必须更新 `docs/AI/SESSION_STATE.md` 并按标准交接格式输出**。

---

## 2. 事实来源 (Sources of Truth)

AI Agent 的所有决策与工作必须基于以下事实来源，不得凭空假设：

| 事实类型 | 文件路径 | 说明 |
| :--- | :--- | :--- |
| **开发指令与流程** | [`docs/AI_AGENT_PROMPT.md`](file:///ssd0/git/EVMEventLake/docs/AI_AGENT_PROMPT.md) | 代理开发工作守则与流程规范 |
| **项目规则** | [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md) | 本文件 |
| **总目标** | [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md) | 系统愿景、架构定位与阶段目标 |
| **任务索引看板** | [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md) | 任务列表、流转状态与看板 |
| **当前会话状态** | [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md) | 跨 Session 恢复与交接事实依据 |
| **具体任务定义** | `docs/AI/tasks/TASK-xxx.md` | 细粒度任务详情（参考 [`TASK-TEMPLATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-TEMPLATE.md)） |
| **系统架构说明** | [`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md) | 模块边界、双存储模式与关键数据流 |
| **关键架构决策** | [`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md) | 技术与架构决策记录（ADR） |

---

## 3. 每次 Session 启动必经流程

1. 确认当前目录为项目根目录。
2. 查看当前仓库状态：`git status --short`。
3. 读取 [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)。
4. 读取 [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)、[`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md) 和 [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)。
5. 若有未完成的 `IN_PROGRESS` Task，优先恢复；否则按顺序选择依赖已满足的 `TODO` Task。
6. 输出修改前执行计划，确认范围后再行编码。
