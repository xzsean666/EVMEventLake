# 任务索引表 (TASK INDEX)

本文档是项目中所有任务（Task）的唯一索引与生命周期看板。

---

## 1. 任务流转规范

任务状态只能按以下规则变化：

```text
TODO -> IN_PROGRESS -> REVIEW -> DONE
                    \-> BLOCKED
```

- **TODO**：已定义且依赖明确，尚未开始。
- **IN_PROGRESS**：当前会话（Session）正在执行（**一次最多一个**）。
- **REVIEW**：代码与文档已完成，等待验证或人工审查。
- **DONE**：验收标准全部满足，测试实际运行通过，文档与交接已更新。
- **BLOCKED**：缺少必要外部条件或权限，已明确记录阻塞原因。

---

## 2. 当前任务看板

### 活跃任务 (In Progress)
- 当前无处于进行中的任务（Active Task: None）。

### 待规划 / 待执行 (TODO)
- 当前无预置任务（遵循“不提前添加任务、不猜测需求”原则，等待用户明确需求后拆分录入）。

### 已完成任务 (DONE)
- **TASK-000**: AI Agent 文档架构与格式规范初始化 (初始化完成，建立规范体系)

---

## 3. 任务定义模板与规范

新增任务必须在 [`docs/AI/tasks/`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/) 目录下创建独立文件，并遵循标准模板：
- 模板文件：[`docs/AI/tasks/TASK-TEMPLATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-TEMPLATE.md)
- 命名规则：`TASK-xxx.md`（三位数字递增，例如 `TASK-001.md`）
- 规模要求：单一明确目标，预估 30~90 分钟，修改文件不超过 5 个实现文件与 3 个测试文件，具备明确的验收标准与可执行验证命令。
