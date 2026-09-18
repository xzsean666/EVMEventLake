# EVMEventLake 核心目标 (GOAL)

## 1. 系统愿景与定位

**EVMEventLake** 是一个专为 EVM 兼容区块链设计的高性能、确定性原始事件日志（Raw Event Logs）采集、索引与检索系统。

核心定位与边界原则：
- **Raw Event Lake 优先**：系统核心专注并保证原始 EVM 日志的完整性、有序性与实时采集，将高复杂度的 ABI 解码与业务语义转换解耦至下游消费者。
- **单体优先与极简存储架构**：
  - **SQLite**：以单文件嵌入式运行在 Rust 单体进程内，作为运维配置、RPC 节点池、订阅、Checkpoints 与鉴权的轻量事务事实源，彻底摆脱外部 RDBMS 容器依赖。
  - **ClickHouse**：系统唯一且强制的分析型原始数据湖引擎，负责承载全链无过滤（`all_events`）海量原始日志与区块交易数据的高通量写入与 DSL 检索。
  - **统一 S3 增量备份恢复**：构建 SQLite 完整快照 + ClickHouse 不可变分片（Parts）增量上传的统一云端备份恢复工具链。
- **Reorg 免疫与确定性**：内置自适应区块拉取、区块哈希一致性校验、基于 Tombstone（`is_removed`）的 Reorg 修复机制，保证日志查询的精确性。

---

## 2. 当前阶段总目标 (Current Milestone Goal)

当前阶段的目标是：**存储架构全面收敛至「SQLite + ClickHouse」并实现 S3 增量备份恢复**。
践行极简 Raw Data Lake 与高可靠运维原则：
1. **全面平移至嵌入式 SQLite**：将 6 张核心控制面与状态表迁移至 SQLite，移除 PostgreSQL 依赖与原始日志分区表管理器，实现零外部 RDBMS 依赖。
2. **ClickHouse 确立为唯一原始日志湖**：统一日志写入与检索链路至 ClickHouse，消除双存储分支与冗余代码。
3. **构建 S3 增量统一备份恢复体系**：实现 SQLite 在线极小快照与 ClickHouse 原生 S3 增量备份联动，提供一键式 Shell 运维脚本。

---

## 3. 任务拆分准则与后续规划

> **注意**：按照开发规范原则，当前不提前预置或猜测具体业务开发 Task。后续具体功能（如区块交易数据采集扩展、动态地址聚合等）将在用户下达明确需求后，严格按照 5~7 项规则拆分入 `docs/AI/tasks/TASK-xxx.md` 并更新 `docs/AI/TASK_INDEX.md`。

---

## 4. 事实来源引用

- 架构详细设计：[`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md)
- 重要架构决策：[`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md)
- 历史升级交接：[`docs/CLICKHOUSE_HANDOVER.md`](file:///ssd0/git/EVMEventLake/docs/CLICKHOUSE_HANDOVER.md)
- 当前工作状态：[`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
