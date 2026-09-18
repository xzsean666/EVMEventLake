# 关键架构与技术决策记录 (DECISIONS)

本文档记录 EVMEventLake 项目的核心技术与架构决策（Architecture Decision Records, ADR）。

---

## ADR-001: 采用 Rust 单体优先架构 (Monolith First)

- **状态**: Accepted
- **背景**: 项目初期与高通量数据采集需要高并发、内存安全和低资源消耗。过早拆分为微服务会导致分布式事务、RPC 运维复杂度和网络开销急剧增加。
- **决策**:
  1. 使用 Rust 编写单体服务（`eventlake`），包含 API、Worker、RPC 池管理与存储写入。
  2. 模块间保持严格的代码边界与 Cognitive Responsibility 分离，禁止隐式全局变量和循环依赖。
- **影响**:
  - 极大简化了部署与端到端测试。
  - 采集、Reorg 回滚与 Checkpoint 推进在一个进程内通过确定性流程协调。

---

## ADR-002: PostgreSQL 作为唯一运维与事务状态事实源

- **状态**: Accepted
- **背景**: 区块链事件采集中，订阅状态、RPC 节点健康度、Checkpoint 进度和 Reorg 检测需要强一致的事务保障（ACID）。
- **决策**:
  1. 无论是否启用 ClickHouse，PostgreSQL 始终负责管理链配置、RPC 端点、订阅意图、Checkpoints、Reorg 观测记录、用户与 API Key。
  2. 杜绝将不可变的分析引擎（如 ClickHouse）用于易变运维状态的更新。
- **影响**:
  - 保证系统崩溃恢复时状态清晰，Checkpoint 绝不超前推进。

---

## ADR-003: 引入 ClickHouse 作为可选的海量原始日志分析引擎

- **状态**: Accepted
- **背景**: 在全链无过滤采集（`all_events`）或高吞吐合约采集场景下，PostgreSQL 的写入吞吐和存储空间成本将成为瓶颈。
- **决策**:
  1. 提供可选编译特征 `--features clickhouse` 与环境变量 `EVENTLAKE_CLICKHOUSE_ENABLED`。
  2. 原始日志采用 `ReplacingMergeTree(stored_at)`，主键为 `(chain_id, block_number, transaction_hash, log_index)`。
  3. 为 `topic0`~`topic3` 建立 Bloom Filter 索引以加速点查与范围匹配。
  4. 遇到链上 Reorg 时，写入更新版本 Tombstone（`is_removed = true`），查询统一使用 `FINAL` 引擎。
- **影响**:
  - 支撑亿级/百亿级 EVM 事件的高效摄入与毫秒级 DSL 检索。
  - ClickHouse 写入失败时不推进 Checkpoint，下次循环自动幂等重试。

---

## ADR-004: 核心服务聚焦 Raw Event Lake，解耦应用层 ABI 解码

- **状态**: Accepted
- **背景**: 早期架构在后台运行实时 ABI 解码器（Decoder）并维护解码事件索引。但生产实践中，合约 ABI 经常升级、多版本并存，且不同业务方关心的事件字段完全不同。在采集端强行同步解码会导致阻塞整个采集管线。
- **决策**:
  1. 核心采集管线精简为 Raw Event Lake，专注于高效收集、持久化、清洗和索引原始日志（topics 与 data）。
  2. 解除后台 Decoder 强制启动逻辑，原有解码表保留为历史兼容读取，新数据不再写入解码表。
  3. ABI 解析与领域数据建模交由下游消费者或独立服务异步按需消费。
- **影响**:
  - 采集吞吐量大幅提升，消除了 ABI 缺失或格式错误对采集管线的阻断。

---

## ADR-005: 严格白名单驱动的统一 Search DSL

- **状态**: Accepted
- **背景**: 允许外部用户通过 REST API 灵活查询日志，必须防御 SQL 注入、跨链无界全表扫描以及高开销的未索引字段检索。
- **决策**:
  1. 设计严格的白名单 Search DSL，强制要求提供合法的 `chain_id eq` 过滤条件。
  2. 仅允许在预定义的字段（`block_number`, `contract_address`, `transaction_hash`, `topic0`~`topic3`）上应用受限操作符（`eq`, `in`, `gte`, `lte`, `gt`, `lt`）。
  3. 编译阶段对 topic 格式做严格 32 字节十六进制正规化校验。
- **影响**:
  - 保证所有生成的 SQL 都能命中 ClickHouse 或 PostgreSQL 索引，杜绝慢查询拖垮集群。

---

## ADR-006: 规范化 AI Agent 工程开发规范体系 (`docs/AI/*`)

- **状态**: Accepted
- **背景**: 多个 AI 交互 Session 容易因为上下文断裂、缺乏全局规则而产生“大包大揽”、“越权修改无关文件”、“未经测试声称通过”等工程失控问题。
- **决策**:
  1. 确立 `docs/AI_AGENT_PROMPT.md` 作为顶层开发准则。
  2. 建立 `docs/AI/` 统一事实来源：`GOAL.md`、`TASK_INDEX.md`、`SESSION_STATE.md`、`ARCHITECTURE.md`、`DECISIONS.md`，以及 `tasks/TASK-xxx.md`。
  3. 严格践行：“一次一个 Task、单 Session 最多完成一个 Task、必须实际执行测试验证、每次结束更新 SESSION_STATE”。
- **影响**:
  - 确保跨 Session 开发具备高度确定性、可维护性与可复现性。

---

## ADR-007: 存储层全面收敛至 SQLite + ClickHouse 唯一数据湖并支持 S3 增量备份恢复

- **状态**: Accepted
- **背景**: 原有架构使用 PostgreSQL 管理控制面元数据并在未开启 ClickHouse 时存储原始日志。这导致部署必须依赖外部 PostgreSQL 容器与数据库运维，且代码中存在双存储分支。随着系统定位为高性能 Raw Event Lake，海量数据全量依托 ClickHouse，PostgreSQL 仅用于几十条元数据，属于重型过度设计。
- **决策**:
  1. **彻底移除 PostgreSQL 依赖**：元数据与控制面（链配置、RPC 节点池、订阅、Checkpoints、鉴权、区块同步状态共 6 张核心表）全面平移至嵌入式 **SQLite**（WAL 模式 + 强一致事务），以单文件形式嵌入 Rust 单体进程，实现零外部 RDBMS 依赖。
  2. **ClickHouse 作为唯一海量数据湖引擎**：移除 `eventlake_raw_logs` 在关系数据库中的存储与分区管理器，所有原始事件、区块与交易数据统一由 ClickHouse 承接。
  3. **统一 S3 增量备份与恢复体系**：
     - SQLite 在线生成极小单文件快照（`VACUUM INTO`，压缩后几百 KB），随每次备份完整保存。
     - ClickHouse 利用不可变分片（Immutable Parts）特性，通过原生 SQL `BACKUP ... TO S3(...) SETTINGS base_backup = ...` 或专用工具执行极速增量推送。
     - 提供开箱即用的一键运维备份与恢复 Shell 脚本，支持本地与云端 S3（含 Cloudflare R2 / MinIO）增量与全量备份及灾难恢复。
- **影响**:
  - 架构极度精简，部署形态从“双外部数据库依赖”降维为“单内置 SQLite + 唯一 ClickHouse 数据湖”。
  - 彻底消除 PostgreSQL 分区表维护开销与双存储适配分支。
  - 实现了分钟级甚至秒级的云端增量备份与无缝断点恢复。

---

## ADR-008: ClickHouse 写入碎片（Parts）防堵塞调优与系统日志轻量化治理

- **状态**: Accepted
- **背景**:
  1. ClickHouse 默认为每次查询和写入生成全量系统日志（`system.query_log`、`system.part_log`、`system.trace_log` 等）且默认保留期长。在区块链数据高频持续写入场景下，系统表膨胀极快，往往在数日内体积远超业务数据本身，极易导致存储爆满。
  2. ClickHouse MergeTree 每次 INSERT 生成独立 Part 并由后台 Merge 异步合并。当并发采集 Worker 以微批（Micro-batching）高频写入时，碎片累积速度可能超过 Merge 速度。一旦达到 `parts_to_delay_insert`（默认 150），写入被强制休眠拖慢；达到 `parts_to_throw_insert`（默认 300），ClickHouse 会抛出 `Too many parts` 异常，导致采集服务卡死或崩溃重试。
- **决策**:
  1. **开启原生异步聚合写入（`async_insert` + `wait_for_async_insert`）**：
     - 在客户端连接全局注入 `async_insert = 1`、`wait_for_async_insert = 1` 与 `async_insert_busy_timeout_ms = 200`。
     - 服务端在内存中对微批数据自动攒批为健康的大 Part 再落盘，彻底终结碎片堆积；同步等待刷盘保证 SQLite Checkpoint 推进的安全一致性。
  2. **从源头切断写入查询日志（`log_queries = 0`）**：
     - 采集管道的客户端连接与 Profile 默认设置 `log_queries = 0`，防止频繁的数据插入操作向 `system.query_log` 灌入巨量日志。
  3. **DDL 表级提升 Part 容忍度**：
     - `raw_logs`、`blocks`、`transactions` 表级 `SETTINGS` 配置 `parts_to_delay_insert = 300`、`parts_to_throw_insert = 600`、`max_delay_to_insert = 1`，避免低阈值过早阻塞。
  4. **容器化配置挂载与系统日志生命周期截断**：
     - 维护 `clickhouse/config.d/system_logs.xml` 与 `clickhouse/users.d/tuning.xml` 并挂载至容器。
     - 移除 `trace_log`；将 `query_log`、`part_log`、`text_log`、`metric_log` 的 TTL 统一截断为 1~2 天自动淘汰，调优后台 `background_pool_size = 16`。
- **影响**:
  - 彻底根除“ClickHouse 运行久了系统日志比业务数据还大”的隐患，磁盘占用降低 80%+。
  - 高并发与微批采集下不再发生 `Too many parts` 写入中断或被强制 delay 卡死。


