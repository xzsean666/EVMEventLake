# TASK-009: 存储层全面迁移至 SQLite 元数据引擎 + ClickHouse 唯一数据湖

## Objective
依据 ADR-007 架构决策，彻底剥离外部 PostgreSQL 依赖，将系统的元数据与运维控制面（6 张核心表）全面平移至嵌入式 SQLite；移除 PostgreSQL 原始日志表及其分区管理器，将 ClickHouse 确立为系统唯一且强制的原始事件日志（Raw Event Logs）与区块交易数据湖引擎。

## Scope
- **包含**：
  1. **依赖重构 (`Cargo.toml`)**：
     - 将 `sqlx` 特征从 `postgres` 替换为 `sqlite`（保留 `runtime-tokio`, `tls-rustls`, `uuid`, `chrono`, `json`, `migrate`, `macros`）。
     - 将 `clickhouse` 与 `time` 提升为核心默认依赖，消除 optional feature 编译开关。
  2. **基线迁移脚本 SQLite 化 (`migrations/202609180001_initial_schema.sql`)**：
     - 移除已无用的 `eventlake_raw_logs` 表及其默认分区与索引。
     - 保留 6 张核心表（`eventlake_chains`, `eventlake_rpc_endpoints`, `eventlake_subscriptions`, `eventlake_block_checkpoints`, `eventlake_api_keys`, `eventlake_block_transaction_sync_state`）。
     - 调整字段类型（`TIMESTAMPTZ` -> `DATETIME` / `TEXT`，`UUID` -> `TEXT`，`JSONB` -> `TEXT`，`BIGINT` -> `INTEGER`）。
     - 调整约束与触发器/默认值（`now()` -> `CURRENT_TIMESTAMP`）。
  3. **数据层与配置层改造**：
     - `src/configuration/mod.rs`：数据库配置由 `postgres://...` 默认切换为 `sqlite://./data/eventlake.db?mode=rwc`。
     - `src/database/mod.rs`：使用 `SqlitePool` 与 `SqlitePoolOptions`，默认启用 `PRAGMA journal_mode = WAL;` 与 `PRAGMA busy_timeout = 5000;`。
     - `src/app/application_state.rs`：将应用状态中的连接池替换为 `SqlitePool`。
  4. **业务模块 SQL 语句平移**：
     - 调整 `chains`, `rpc_pool`, `subscriptions`, `reorg`, `auth`, `block_transaction::state` 中的 `PgPool` -> `SqlitePool`。
     - 替换 PostgreSQL 特有函数（如 `now()` -> `CURRENT_TIMESTAMP`）。
  5. **精简与移除冗余代码**：
     - 移除 `src/indexing/partition_manager.rs` 及相关的后台任务调度（由于不再存在 PG 原始日志表）。
     - `src/search/mod.rs` 与 `src/collector/worker.rs` 移除针对 PG 原始日志的兜底逻辑与条件编译宏，统一直接对接 ClickHouse。
  6. **测试套件与验证**：
     - 将相关集成测试适配为 SQLite 本地测试（使用临时文件或内存数据库）。
     - 运行全量 `cargo check` 与测试套件。
- **不包含**：
  1. 不修改上层对外公开的 REST API 契约和 Search DSL 语法。
  2. 不在本任务中实现 S3 增量备份与恢复脚本（由 TASK-010 实现）。

## Allowed Files
- `Cargo.toml`
- `migrations/202609180001_initial_schema.sql`
- `src/configuration/mod.rs`
- `src/database/mod.rs`
- `src/app/mod.rs`
- `src/app/application_state.rs`
- `src/chains/mod.rs`
- `src/rpc_pool/mod.rs`
- `src/rpc_pool/worker.rs`
- `src/subscriptions/mod.rs`
- `src/reorg/mod.rs`
- `src/auth/mod.rs`
- `src/collector/worker.rs`
- `src/search/mod.rs`
- `src/block_transaction/state.rs`
- `src/block_transaction/worker.rs`
- `src/indexing/mod.rs`
- `src/indexing/partition_manager.rs`
- `tests/*`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/tasks/TASK-009.md`
- `docs/AI/tasks/TASK-010.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-008 (已完成)
- 架构决策：ADR-007

## Inputs and Outputs
- **Inputs**: ADR-007 架构决策，用户关于收敛为单一 SQLite + ClickHouse 架构的指令。
- **Outputs**: 零外部 PostgreSQL 依赖的 Rust 单体服务，全量通过的测试套件与干净对齐的纯 SQLite 基线迁移。

## Acceptance Criteria
- [x] 1. `Cargo.toml` 移除 `postgres` 特征，引入 `sqlite` 特征；`clickhouse` 与 `time` 成为核心必须依赖。
- [x] 2. `migrations/202609180001_initial_schema.sql` 成功适配为纯 SQLite 语法，移除 `eventlake_raw_logs` 表，支持全新 SQLite 库自动迁移初始化。
- [x] 3. 核心 6 个业务模块及鉴权全部切换为 `SqlitePool`，消除所有 `now()` 等 PG 方言不兼容点。
- [x] 4. 彻底移除 `src/indexing/partition_manager.rs`，消除 PG 原始日志表分区管理开销。
- [x] 5. `collector/worker.rs` 与 `search/mod.rs` 消除条件编译宏，统一直接对接 ClickHouse。
- [x] 6. `cargo check --all-targets` 零告警通过，单元测试及集成测试通过。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version --lib
cargo test --ignore-rust-version --test validation_tests
cargo test --ignore-rust-version --test rpc_pool_cooldown_test
cargo test --ignore-rust-version --test collector_concurrency_tests
cargo test --ignore-rust-version --test search_dsl_tests
cargo test --ignore-rust-version --test e2e_real_database_tests
cargo test --ignore-rust-version
```

## Risks and Assumptions
- **假设**：无需兼容旧 PostgreSQL 历史数据；ClickHouse 作为唯一原始事件湖。
- **风险**：SQLite 在多任务并发写入时可能产生锁等待，通过显式配置 WAL 模式与 `busy_timeout(5s)` 解决。

## Status
DONE
