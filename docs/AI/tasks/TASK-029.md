# TASK-029: 扩展实用区块与交易分析 API (时间查块、时间区间、交易确认数、地址画像与合约部署)

## Objective
基于 ClickHouse `blocks` 与 `transactions` 现有数据集，扩展落地 5 个高价值、方便实用且低复杂度的核心 REST API，满足下游在时间与区块互转、轻量交易确认数与状态、地址首末活跃画像及合约部署（打新）发现等关键业务场景的需求。

## Scope
- 包含：
  - 在 `src/clickhouse/block_transaction.rs` 中新增 5 个专用 ClickHouse 查询函数：
    - `get_block_by_timestamp(chain_id, timestamp, closest)`
    - `get_blocks_time_range(chain_id, start_time, end_time)`
    - `get_transaction_status(chain_id, tx_hash)`
    - `get_address_profile(chain_id, address)`
    - `get_contract_deployments(chain_id, creator, limit, cursor)`
  - 在 `src/block_transaction/api.rs` 中新增 5 个 Axum REST 路由及 OpenAPI 规范定义：
    - `GET /api/chains/{chain_id}/block-by-time`
    - `GET /api/chains/{chain_id}/blocks-time-range`
    - `GET /api/chains/{chain_id}/transactions/{tx_hash}/status`
    - `GET /api/chains/{chain_id}/addresses/{address}/profile`
    - `GET /api/chains/{chain_id}/deployments`
  - 编写完备的单元测试与 ClickHouse 集成测试，验证入参校验、时间边界、排序与游标分页。
- 不包含：
  - 修改已有 `raw_logs` 采集器或已有的基础点查接口。
  - 引入新的外部数据库或重量级复杂中间件。

## Allowed Files
- `src/clickhouse/block_transaction.rs`
- `src/block_transaction/api.rs`
- `tests/block_transaction_test.rs`
- `tests/clickhouse_integration_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/tasks/TASK-029.md`

## Dependencies
- 前置任务：TASK-028 (已完成)
- 外部依赖：ClickHouse 实例运行中

## Inputs and Outputs
- **Inputs**:
  - `chain_id`: EVM 链 ID
  - `timestamp` / `start_time` / `end_time`: 秒级 Unix 时间戳
  - `closest`: `before` (默认) 或 `after`
  - `tx_hash`: 32 字节交易哈希
  - `address`: 20 字节 EVM 地址
  - `creator`: 20 字节合约创建者地址（可选）
  - `limit` / `cursor`: 分页控制
- **Outputs**:
  - `BlockDetailResponse`: 最近区块详情
  - `BlockTimeRangeResponse`: 起止区块高度与块数统计
  - `TransactionStatusResponse`: 交易状态、所在高度、最新高度与确认数
  - `AddressProfileResponse`: 首次/最后活跃高度与时间、收发交易笔数、最新 Nonce
  - `Vec<TransactionDetailResponse>`: 合约部署交易列表（带分页游标）

## Acceptance Criteria
- [x] 验收标准 1：`get_block_by_timestamp` 支持 `closest=before`（$\le timestamp$）和 `closest=after`（$\ge timestamp$），精确返回最近的规范区块（自动过滤 `is_canonical = false`）。
- [x] 验收标准 2：`get_blocks_time_range` 在指定时间窗口内返回最小高度、最大高度与总出块数；无块时间段返回友好空状态。
- [x] 验收标准 3：`get_transaction_status` 返回交易状态（成功/失败/未确认）、Gas 消耗、所在区块及结合当前链上 Head 计算出的准确确认数（`confirmations`）。
- [x] 验收标准 4：`get_address_profile` 单次查询统计出地址的 `first_block`, `last_block`, `sent_tx_count`, `received_tx_count`, `last_nonce`。
- [x] 验收标准 5：`get_contract_deployments` 准确筛选 `to_address IS NULL` 的部署交易，支持按 `creator` 过滤与 keyset 游标分页。
- [x] 验收标准 6：全部单元测试与集成测试通过，`cargo check --ignore-rust-version --all-targets` 与 `cargo test --ignore-rust-version` 运行无报错（全库 64 个测试 100% 通过）。

## Verification Commands
```bash
cargo test --ignore-rust-version --test block_transaction_test
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true cargo test --ignore-rust-version --test clickhouse_integration_tests
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version
```

## Risks and Assumptions
- 风险：极低，仅在只读查询层新增函数与路由，不影响任何后台写入流水线。
- 假设：ClickHouse 表结构维持原有定义，`blocks` 与 `transactions` 已有相应索引。

## Status
DONE
