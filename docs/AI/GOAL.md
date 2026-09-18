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

当前阶段的目标是：**系统全面精简与性能飞跃重构**。
全面践行 Raw Data Lake 原则，坚决剔除遗留的 Decoder、ABI Registry 与 Explorers 僵尸代码；精简存储层与 Reorg 流程消除写入放大；实现 RPC 节点池阶梯式退避熔断冷却（1m -> 5m -> 24h）与内存路由缓存；对核心日志采集流水线实施全异步并发改造，打造极简、高吞吐、低延迟的确定性 EVM 数据湖引擎。

主要目标要求：
1. **代码与功能深度瘦身**：彻底删除未激活的 Decoder、不再匹配的 ABI Registry 与 Explorers 模块，移除无效的第三方依赖与内存缓存，精简 ClickHouse 废弃表。
2. **RPC 阶梯退避与零打库路由**：实现失败阶梯冷却机制（1分钟 -> 5分钟 -> 最大24小时，成功即清空），建立内存路由缓存减少 PostgreSQL 热点查询。
3. **采集流水线高并发改造**：将原本串行的多链/多订阅收集改造为基于 `JoinSet` 与 `Semaphore` 的高并发异步隔离调度。
4. **消除 Reorg 性能拖累**：剔除已废弃表在分叉回退时的 Tombstone 放大与全表 Group By 聚合查询。

---

## 3. 任务拆分准则与后续规划

> **注意**：按照开发规范原则，当前不提前预置或猜测具体业务开发 Task。后续具体功能（如区块交易数据采集扩展、动态地址聚合等）将在用户下达明确需求后，严格按照 5~7 项规则拆分入 `docs/AI/tasks/TASK-xxx.md` 并更新 `docs/AI/TASK_INDEX.md`。

---

## 4. 事实来源引用

- 架构详细设计：[`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md)
- 重要架构决策：[`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md)
- 历史升级交接：[`docs/CLICKHOUSE_HANDOVER.md`](file:///ssd0/git/EVMEventLake/docs/CLICKHOUSE_HANDOVER.md)
- 当前工作状态：[`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
