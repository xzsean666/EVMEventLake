# TASK-006: 审计优化实施（清理残留废弃表查询、轻量化Reorg流程、补齐交易收据API与收敛输入校验）

## Objective
对系统全面审计中发现的高价值优化点进行落地实施：彻底清理 ClickHouse 模块中残留的废弃表查询与孤儿代码；精简 PostgreSQL 分叉回退（Reorg）逻辑以消除空表聚合开销；在 REST API 交易详情响应中补齐已持久化的交易收据与 L2 燃气字段；严格收敛十六进制地址/哈希前缀校验防御畸形输入；清理遗留的 `decode_batch_size` 配置并对齐 `.env.example`。

## Scope
- 包含：
  - 移除 `src/clickhouse/mod.rs` 中查询已删除表 `decoded_events` / `address_index` 的废弃函数与结构体。
  - 精简 `src/reorg/mod.rs` 的分叉回退逻辑，移除对已停用表的无意义扫描、更新与全表聚合操作。
  - 在 `src/block_transaction/api.rs` 的 `TransactionDetailResponse` 中补齐 `status`, `gas_used`, `effective_gas_price`, `l1_fee` 字段。
  - 改造 `src/shared/validation.rs` 中的十六进制校验逻辑，严格单次剥离 `0x`/`0X` 前缀，阻断重复前缀绕过。
  - 移除 `src/configuration/mod.rs` 中遗留的 `decode_batch_size` 配置，同步更新全测试套件与 `.env.example`。
  - 修正 `tests/e2e_real_database_tests.rs` 中已废弃的 `/api/search` 断言。
- 不包含：
  - 引入新的重型依赖。
  - 变更数据库核心迁移表结构或破坏 Raw Data Lake 架构定位。

## Allowed Files
- `src/clickhouse/mod.rs`
- `src/reorg/mod.rs`
- `src/block_transaction/api.rs`
- `src/shared/validation.rs`
- `src/configuration/mod.rs`
- `.env.example`
- `tests/validation_tests.rs`
- `tests/e2e_real_database_tests.rs`
- `tests/clickhouse_integration_tests.rs`
- `tests/live_real_evm_data_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-005 (已完成)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 审计发现的 6 项优化需求。
- **Outputs**: 精简且一致的代码库、完整的交易详情 API、健壮的校验逻辑、同步的配置文档与全部通过的回归测试。

## Acceptance Criteria
- [x] 标准 1：`src/clickhouse/mod.rs` 中不再包含 `decoded_events` 和 `address_index` 废弃表查询逻辑，编译零警告。
- [x] 标准 2：`src/reorg/mod.rs` 的 `invalidate_from_block` 仅对 `eventlake_raw_logs` 和 `eventlake_subscriptions` 进行必要维护，消除 `refresh_contract_registry` 全表扫描。
- [x] 标准 3：`TransactionDetailResponse` 正确包含并序列化 `status`, `gas_used`, `effective_gas_price`, `l1_fee`。
- [x] 标准 4：`normalize_address`、`normalize_topic`、`normalize_hash` 正确拒绝 `"0x0x..."` 等多重前缀畸形输入。
- [x] 标准 5：`decode_batch_size` 彻底移除，`.env.example` 补齐并发与区块交易配置项。
- [x] 标准 6：全库单元测试与集成测试（默认及 ClickHouse 特性）全部实际运行通过。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo check --ignore-rust-version --features clickhouse --all-targets
cargo test --ignore-rust-version --features clickhouse --lib
cargo test --ignore-rust-version --test validation_tests
cargo test --ignore-rust-version --features clickhouse --test block_transaction_test
cargo test --ignore-rust-version --test rpc_pool_cooldown_test
cargo test --ignore-rust-version --test collector_concurrency_tests
cargo test --ignore-rust-version --test search_dsl_tests
```

## Risks and Assumptions
- 风险：移除 `decode_batch_size` 需要同步更新测试中的配置构造器，已完成全套件同步，零破坏。
- 假设：已有 API 消费者若依赖 `TransactionDetailResponse`，新增 4 个可选字段具备完全向下兼容性。

## Status
DONE
