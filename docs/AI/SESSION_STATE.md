# 会话状态记录 (SESSION STATE)

本文档记录当前开发会话的状态，是跨 Session 恢复工作的直接依据。

---

## 1. 核心状态概要

- **当前 Goal**: 规范文档架构和格式，建立 `docs/AI/` 事实来源体系，并彻底清理所有冗余、过时与过渡性历史文档。
- **当前 Task**: 深度文档清理与体系收敛 (Deep Documentation Cleanup)
- **当前状态**: `DONE`

---

## 2. 本次会话完成内容 (Accomplished Work)

1. **确立规范化 AI 事实来源体系 (`docs/AI/`)**:
   - [`docs/AI_AGENT_PROMPT.md`](file:///ssd0/git/EVMEventLake/docs/AI_AGENT_PROMPT.md): AI Agent 顶层工程开发工作准则。
   - [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md): 系统定位（Raw Event Lake 优先、双存储模式）与阶段总目标。
   - [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md): 任务索引与生命周期看板（当前无悬挂任务）。
   - [`docs/AI/tasks/TASK-TEMPLATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-TEMPLATE.md): 标准任务模板。
   - [`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md): 权威系统架构说明（模块边界、双存储一致性、Reorg Tombstones、统一 Search DSL）。
   - [`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md): 架构决策记录（ADR-001 ~ ADR-006）。
   - [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md): 根目录 Agent 引导规范。
2. **彻底清理所有冗余、历史过渡与重复文档**:
   - 移除重复旧架构说明：`docs/ARCHITECTURE.md`（统一归入 `docs/AI/ARCHITECTURE.md`）。
   - 移除已完成特性的历史升级方案与交接手记：`docs/CLICKHOUSE_UPGRADE.md`、`docs/CLICKHOUSE_HANDOVER.md`。
   - 移除已落地的升级过程子目录：`docs/block-transaction-upgrade/`、`docs/dynamic-address-aggregation-upgrade/`。
   - 移除容量预算草案与外部提示词：`docs/evm-block-transaction-data-collection-architecture.md`、`docs/RPC_SEEDS_PROMPT.md`。
   - 移除旧版交接与脚手架：`Agent.md`、`docs/nextsession.md`、`docs/RUST_DOCKER_DEPLOYMENT.md`。
3. **保留不可替代的核心工程文档**:
   - [`docs/SPEC.md`](file:///ssd0/git/EVMEventLake/docs/SPEC.md): 核心产品模型与系统设计规格。
   - [`docs/BUILD.md`](file:///ssd0/git/EVMEventLake/docs/BUILD.md): 本地编译、静态检查与测试环境指南。
   - [`docs/USAGE.md`](file:///ssd0/git/EVMEventLake/docs/USAGE.md): API 接口调用与使用手册（已修复链接）。
   - [`docs/DEPLOYMENT.md`](file:///ssd0/git/EVMEventLake/docs/DEPLOYMENT.md): 多形态 Docker 生产部署指南。
   - [`docs/EXTERNAL_DOCS.md`](file:///ssd0/git/EVMEventLake/docs/EXTERNAL_DOCS.md): 外部官方文档与技术栈索引。
   - 同步修正了 [`README.md`](file:///ssd0/git/EVMEventLake/README.md) 的索引链接。

---

## 3. 文件变动清单

### 新建文件 (Created Files)
- `AGENTS.md`
- `docs/AI_AGENT_PROMPT.md`
- `docs/AI/GOAL.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/ARCHITECTURE.md`
- `docs/AI/DECISIONS.md`
- `docs/AI/tasks/TASK-TEMPLATE.md`

### 彻底清理的文件/目录 (Deleted)
- `Agent.md`
- `docs/ARCHITECTURE.md`
- `docs/nextsession.md`
- `docs/RUST_DOCKER_DEPLOYMENT.md`
- `docs/CLICKHOUSE_UPGRADE.md`
- `docs/CLICKHOUSE_HANDOVER.md`
- `docs/RPC_SEEDS_PROMPT.md`
- `docs/evm-block-transaction-data-collection-architecture.md`
- `docs/block-transaction-upgrade/`
- `docs/dynamic-address-aggregation-upgrade/`

### 调整修正的文件 (Modified Files)
- `README.md`
- `docs/USAGE.md`

---

## 4. 已运行的验证命令及结果

```bash
# 验证剩余文件列表
find docs -type f | sort

# 验证 git 状态
git status --short
```
验证结果：`docs/` 目录精简至 12 个职责绝对清晰、互不重叠的核心文档；Git 变更仅涉及文档清理与规范重塑，代码零干扰。

---

## 5. 未解决问题 (Known Issues)

- 无。

---

## 6. 风险和假设 (Risks and Assumptions)

- **假设**：所有历史已完成特性的核心设计与参数（如 ClickHouse 写入一致性、多地址合乘采集 `max_batch_addresses`、区块交易数据模型）均已在 `docs/AI/ARCHITECTURE.md`、`docs/SPEC.md` 和实际源码中完整保全。
- **风险**：无。

---

## 7. 下一步计划 (Next Task)

- **下一步应执行的任务**: 等待用户下达具体的业务需求或开发指令后，按规范在 `docs/AI/tasks/` 下创建 `TASK-001.md` 并开始实施。
- **下一次 Session 应先读取的文件**:
  1. [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)
  2. [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)
  3. [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md)
  4. [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
