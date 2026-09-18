# TASK-008: 压平重构 PostgreSQL 数据库迁移 (Squash Migrations) 并清理废弃表残留

## Objective
在无需向后兼容历史遗留部署的前提下，对 PostgreSQL 数据库迁移体系执行彻底的压平重构（Squash Migrations）：将历史 7 个修补演进迁移脚本合并收敛为单一最新基线迁移 `202609180001_initial_schema.sql`，彻底剔除已废弃的旧解码器与事件索引表（7 张废弃表）；同步清理 `src/subscriptions/mod.rs` 与测试套件中残留的废弃表查询与插入逻辑，使代码库与底层数据湖架构保持纯粹一致。

## Scope
- **包含**：
  1. 重写 `migrations/`：删除 7 个历史迁移文件，创建单一统一基线迁移 `202609180001_initial_schema.sql`，仅保留 7 张核心业务表（`eventlake_chains`, `eventlake_rpc_endpoints`, `eventlake_subscriptions`, `eventlake_block_checkpoints`, `eventlake_raw_logs`, `eventlake_api_keys`, `eventlake_block_transaction_sync_state`）及其最新字段、约束与高效索引。
  2. 彻底移除 `src/subscriptions/mod.rs` 中对 `eventlake_contract_registry` 的无用插入逻辑与 `record_contract` 辅助函数。
  3. 移除 `src/subscriptions/mod.rs` 中 `clear_retrying_subscription` 对已废弃表 `eventlake_decode_queue` 的无意义子查询。
  4. 清理 `tests/e2e_real_database_tests.rs` 中的废弃表清理（`TRUNCATE` / `DROP TABLE`）列表以及对 `eventlake_decode_queue` 的行数断言。
  5. 执行全量编译检查与全套测试套件验证。
- **不包含**：
  1. 不破坏现有的 REST API 结构与字段。
  2. 不影响 ClickHouse 存储与查询逻辑。

## Allowed Files
- `migrations/*`
- `src/subscriptions/mod.rs`
- `tests/e2e_real_database_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/tasks/TASK-008.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-007 (已完成)
- 外部依赖：无

## Inputs and Outputs
- **Inputs**: 架构精简需求及用户“无需兼容已部署实例”指令。
- **Outputs**: 结构纯净的单一基线迁移文件、完全无废弃表调用的 Rust 代码与通过的回归测试。

## Acceptance Criteria
- [x] 1. `migrations/` 目录下仅存在单一基线迁移 `202609180001_initial_schema.sql`，无任何旧迁移文件。
- [x] 2. 基线迁移完整覆盖当前系统所需的全部 7 张核心表、种子数据、检查约束、复合索引与分区定义。
- [x] 3. `src/subscriptions/mod.rs` 中零 `eventlake_contract_registry` 和 `eventlake_decode_queue` 引用。
- [x] 4. `tests/e2e_real_database_tests.rs` 清理废弃表引用，迁移与重置逻辑与新结构对齐。
- [x] 5. 静态检查 `cargo check --all-targets` 与 `cargo check --features clickhouse --all-targets` 零告警。
- [x] 6. 单元与集成测试运行通过。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo check --ignore-rust-version --features clickhouse --all-targets
cargo test --ignore-rust-version --features clickhouse --lib
cargo test --ignore-rust-version --test validation_tests
cargo test --ignore-rust-version --test collector_concurrency_tests
cargo test --ignore-rust-version --test rpc_pool_cooldown_test
```

## Risks and Assumptions
- **假设**：无需兼容历史已迁移的数据库实例，新数据库初始化将直接应用单一基线迁移。
- **风险**：无。

## Status
DONE
