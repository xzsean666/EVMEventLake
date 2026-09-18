# EVMEventLake 核心目标 (GOAL)

## 1. 系统愿景与定位

**EVMEventLake** 是一个专为 EVM 兼容区块链设计的高性能、确定性原始事件日志（Raw Event Logs）采集、索引与检索系统。

核心定位与边界原则：
- **Raw Event Lake 优先**：系统核心专注并保证原始 EVM 日志的完整性、有序性与实时采集，将高复杂度的 ABI 解码与业务语义转换解耦至下游消费者。
- **单体优先与双存储架构**：
  - **PostgreSQL**：作为系统运维状态、配置、链与 RPC 管理、订阅、Checkpoints、Reorg 观测与身份认证的唯一事务事实源。
  - **ClickHouse（可选引擎）**：作为海量原始事件日志存储与快速检索引擎，支持全链无过滤（`all_events`）采集与高并发 DSL 查询。在禁用 ClickHouse 时，原始日志保留在 PostgreSQL 适用轻量部署。
- **Reorg 免疫与确定性**：内置自适应区块拉取、区块哈希一致性校验、基于 Tombstone（`is_removed`）的 Reorg 修复机制，保证日志查询的精确性。

---

## 2. 当前阶段总目标 (Current Milestone Goal)

当前阶段的目标是：**确立严格的工程开发与文档事实来源规范，保持现有 Rust 单体内核与双存储模式的稳健性，为下一阶段功能演进奠定清晰的基础状态。**

主要目标要求：
1. **统一文档与事实来源**：全面接入 `docs/AI/` 体系，以规范化的工作流指导后续所有开发与重构。
2. **代码可维护与可测试性**：保持现有模块边界（`app`, `api`, `collector`, `database`, `clickhouse`, `subscriptions`, `rpc_pool`, `reorg`, `search`）清晰隔离，杜绝隐式全局状态与跨模块逻辑泄漏。
3. **保持状态明确**：不臆测业务需求，不提前铺设未经验证的复杂抽象。

---

## 3. 任务拆分准则与后续规划

> **注意**：按照开发规范原则，当前不提前预置或猜测具体业务开发 Task。后续具体功能（如区块交易数据采集扩展、动态地址聚合等）将在用户下达明确需求后，严格按照 5~7 项规则拆分入 `docs/AI/tasks/TASK-xxx.md` 并更新 `docs/AI/TASK_INDEX.md`。

---

## 4. 事实来源引用

- 架构详细设计：[`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md)
- 重要架构决策：[`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md)
- 历史升级交接：[`docs/CLICKHOUSE_HANDOVER.md`](file:///ssd0/git/EVMEventLake/docs/CLICKHOUSE_HANDOVER.md)
- 当前工作状态：[`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
