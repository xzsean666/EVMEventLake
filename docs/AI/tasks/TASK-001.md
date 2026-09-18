# TASK-001: 扩展区块交易采集模块以支持交易收据 (Receipts) 与 L2 燃气指标

## Objective
为 EVMEventLake 的 `block_transaction` 采集流水线扩展交易收据（Transaction Receipts）采集能力，通过 `eth_getBlockReceipts` 批量获取交易执行结果，补齐 ClickHouse `transactions` 表中的 `status`（成功/失败状态）、`gas_used`（真实消耗 Gas）、`effective_gas_price` 以及 OP Stack（如 Soneium、Base、Optimism）专有的 `l1_fee` 字段，使底层数据湖能够无缝支撑下游 Gas Rebate 返还结算、真实链上成本分析和交易状态过滤等业务需求。

## Scope
- **包含**：
  1. 在 `src/rpc_pool/evm_rpc_client.rs` 中新增 `eth_get_block_receipts_batch` 批量 JSON-RPC 调用，定义结构体解析收据关键字段（`status`、`gasUsed`、`effectiveGasPrice`、`l1Fee` 等）。
  2. 在 `src/block_transaction/collector.rs` 的批处理流水线中，在拉取 `eth_get_blocks_by_number_batch` 后对齐获取对应区块的收据，并按 `tx_hash` / `transaction_index` 将收据信息挂载至交易对象。
  3. 更新 ClickHouse 表结构定义（`clickhouse/schema.sql`），在 `transactions` 表新增 `status`（Nullable(UInt8)）、`gas_used`（Nullable(String)）、`effective_gas_price`（Nullable(String)）、`l1_fee`（Nullable(String)）字段。
  4. 更新 `src/clickhouse/block_transaction.rs` 中的 `TransactionRow` 数据模型与插入逻辑，兼容新字段。
  5. 增加优雅降级容错机制：当 RPC 节点不支持 `eth_getBlockReceipts` 时记录警告并填 NULL，不阻断主采集流程。
  6. 编写对应的单元测试与集成测试用例。
- **不包含**：
  1. 不包含下游业务层的具体积分计算、TWAB 时间加权余额计算或 Gas 返还规则（严格保持底层通用数据湖定位，业务计算属于下游服务）。
  2. 不包含 Startale App 任务发放或积分相关的业务 API。
  3. 不引入新的第三方数据库（保持现有 PostgreSQL 运行态 + ClickHouse 数据湖模式）。

## Allowed Files
- `src/rpc_pool/evm_rpc_client.rs`
- `src/block_transaction/collector.rs`
- `src/clickhouse/block_transaction.rs`
- `clickhouse/schema.sql`
- `tests/block_transaction_test.rs`
- `tests/clickhouse_integration_tests.rs`
- `tests/live_real_evm_data_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/tasks/TASK-001.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：`TASK-000`（已完成）
- 外部依赖：支持 `eth_getBlockReceipts` 批处理的 EVM / OP Stack RPC 节点（如 Soneium、Base、Optimism、Geth 1.10+、Reth）。

## Inputs and Outputs
- **Inputs**:
  - JSON-RPC 方法：`eth_getBlockReceipts(block_number_hex)`。
  - 待解析收据字段：`status`、`gasUsed`、`effectiveGasPrice`、`cumulativeGasUsed`、`l1Fee`（OP Stack 专有）。
- **Outputs**:
  - 写入 ClickHouse `transactions` 表的高保真交易记录（包含真实执行状态与燃气成本消耗）。

## Acceptance Criteria
- [x] 1. **批量 RPC 调用能力**：`evm_rpc_client` 提供 `eth_get_block_receipts_batch`，单次 HTTP Batch 请求可同时拉取多个区块的所有交易收据。
- [x] 2. **数据精确匹配对齐**：采集器在写入 ClickHouse 前，能够准确根据 `tx_hash` 或 `(block_number, transaction_index)` 将 Receipt 与 Transaction 关联并填充字段。
- [x] 3. **OP Stack 兼容性**：在 OP Stack 链（如 Soneium）环境下，若 Receipt 返回中包含 `l1Fee`，能正确提取并转换为十进制字符串存储。
- [x] 4. **ClickHouse 幂等落库**：`clickhouse/schema.sql` 更新 `transactions` 表结构；`TransactionRow` 正确序列化；`ReplacingMergeTree` 在重试与 Reorg 时维持幂等一致。
- [x] 5. **容错与向后兼容**：若 RPC 节点返回 `MethodNotFound` 或未开启收据批处理接口，采集器降级将收据字段置空（None），并以 Warning 级别记录日志，不导致整批同步失败。
- [x] 6. **编译与质量检查**：通过 `cargo check --features clickhouse`，代码格式符合规范，通过新增的收据解析测试。

## Verification Commands
```bash
# 1. 编译检查（必须启用 clickhouse 特性，若环境 rustc 限制可追加 --ignore-rust-version）
cargo check --ignore-rust-version --features clickhouse

# 2. 运行区块与交易收据相关集成与单元测试
cargo test --ignore-rust-version --features clickhouse --test block_transaction_test
cargo test --ignore-rust-version --features clickhouse evm_rpc_client

# 3. 静态检查全目标覆盖
cargo check --ignore-rust-version --features clickhouse --all-targets
```

## Risks and Assumptions
- **风险**：极少数低配第三方 RPC 节点在并发调用 `eth_getBlockReceipts` 批处理时可能遭遇超时。
  - *应对*：复用已有的 RPC Pool 熔断切换与自适应 batch_size 机制，超时后自动重试并降速。
- **假设**：目标链 Soneium 基于 OP Stack 标准架构，其官方 RPC 节点原生支持 `eth_getBlockReceipts` 且包含 `l1Fee` 字段。

## Status
DONE
