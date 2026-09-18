# TASK-003: 精简 ClickHouse 存储与 Reorg 流程（清理废表定义、消除 Reorg 4倍写入放大与空表 DDL 调度）

## Objective
在 TASK-002 移除应用层解码与 ABI 模块的基础上，进一步清理存储层遗留的废弃表定义（`eventlake_decoded_events`、`eventlake_address_index`、`eventlake_event_field_index`），移除 `src/clickhouse/mod.rs` 中对这些空表的 Reorg Tombstone 插入（消除 Reorg 阶段 4 倍写放大），并精简 PostgreSQL 中的 `create_decoded_partition` 调度，彻底夯实 Raw Lake 轻量底座。

## Scope
- 包含：
  - 更新 `clickhouse/schema.sql`：移除 3 张已废弃表定义及相关索引定义。
  - 修改 `src/clickhouse/mod.rs`：
    - 移除 `invalidate_from_block` 中向 3 张废表写入 Tombstone 的逻辑，仅保留针对 `eventlake_raw_logs` 的 Tombstone 操作。
    - 移除初始化时为废弃表执行的预检和 DDL 逻辑。
  - 修改 `src/indexing/partition_manager.rs`：移除 PostgreSQL 中为 `decoded_events` 创建分区的旧函数及定时调度。
  - 调整 `tests/clickhouse_integration_tests.rs`：移除对废弃表的断言。
- 不包含：
  - RPC 节点冷却机制（由 TASK-004 处理）。
  - 采集器并发改造（由 TASK-005 处理）。

## Allowed Files
- `clickhouse/schema.sql`
- `src/clickhouse/mod.rs`
- `src/indexing/partition_manager.rs`
- `tests/clickhouse_integration_tests.rs`
- `docs/AI/tasks/TASK-003.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-002 (DONE)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 现有的 ClickHouse schema、PostgreSQL 分区管理器及 ClickHouse 存储客户端。
- **Outputs**: 纯净且高效的表结构，消除无谓的 Reorg 写放大与分区维护开销。

## Acceptance Criteria
- [x] `clickhouse/schema.sql` 仅保留 `eventlake_raw_logs`、`blocks`、`transactions` 表及其实时视图。
- [x] `src/clickhouse/mod.rs` 中的 `invalidate_from_block` 不再向废弃表插入 Tombstone 记录。
- [x] `src/indexing/partition_manager.rs` 不再执行 `create_decoded_partition`。
- [x] `cargo check --ignore-rust-version --all-targets` 编译通过。
- [x] `cargo check --ignore-rust-version --features clickhouse --all-targets` 编译通过。
- [x] 全套单元测试与集成测试通过。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo check --ignore-rust-version --features clickhouse --all-targets
cargo test --ignore-rust-version --features clickhouse --lib
cargo test --ignore-rust-version --test search_dsl_tests
cargo test --ignore-rust-version --features clickhouse --test block_transaction_test
```

## Risks and Assumptions
- 风险：若存在老旧环境依赖 ClickHouse 废弃表，历史表会被废弃。已确认用户同意方案 A 全面转向 Raw Lake。
- 假设：ClickHouse 客户端在无废表 DDL 情况下正常启动。

## Status
DONE
