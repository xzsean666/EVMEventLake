# TASK-030: 扩展高级链上分析 API (区块用户 Gas 排行、Gas Oracle、网络统计、巨鲸转账、热门合约与失败交易)

## Objective
基于 ClickHouse `blocks` 与 `transactions` 数据集，扩展实现 6 个高级链上分析与监控 REST API：包含按区块查询各用户消耗 Gas 的排行榜、实时 Gas 预测器（Gas Oracle）、网络实时健康统计（TPS/出块时间/拥堵度）、大额巨鲸转账监控、热门合约排行以及失败交易排查流，全面增强数据湖的分析消费能力。

## Scope
- 包含：
  - 在 `src/clickhouse/block_transaction.rs` 中新增 6 个专用查询函数：
    - `get_block_gas_consumers(chain_id, block_number, limit)`
    - `get_gas_oracle(chain_id)`
    - `get_network_stats(chain_id)`
    - `get_whale_transfers(chain_id, min_value, limit, cursor)`
    - `get_top_contracts(chain_id, window_blocks, limit)`
    - `get_failed_transactions(chain_id, limit, cursor)`
  - 在 `src/block_transaction/api.rs` 中新增 6 个 Axum REST 路由及 OpenAPI 规范定义：
    - `GET /api/chains/{chain_id}/blocks/{block_ref}/gas-consumers`
    - `GET /api/chains/{chain_id}/gas-oracle`
    - `GET /api/chains/{chain_id}/network-stats`
    - `GET /api/chains/{chain_id}/whale-transfers`
    - `GET /api/chains/{chain_id}/top-contracts`
    - `GET /api/chains/{chain_id}/failed-transactions`
  - 编写单元测试与 ClickHouse 存储测试验证其聚合逻辑、边界处理与游标分页。
- 不包含：
  - 修改已有采集器或写入逻辑。

## Allowed Files
- `src/clickhouse/block_transaction.rs`
- `src/clickhouse/mod.rs`
- `src/block_transaction/api.rs`
- `tests/block_transaction_test.rs`
- `tests/clickhouse_integration_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/tasks/TASK-030.md`

## Dependencies
- 前置任务：TASK-029 (已完成)
- 外部依赖：ClickHouse 实例运行中

## Inputs and Outputs
- **Inputs**:
  - `chain_id`: EVM 链 ID
  - `block_ref`: 区块高度或哈希
  - `min_value`: 巨鲸转账最小金额阈值 (wei)
  - `window_blocks`: 热门合约统计的时间窗口区块数 (默认 1000)
  - `limit` / `cursor`: 分页控制
- **Outputs**:
  - `BlockGasConsumersResponse`: 区块内各用户 Gas 消耗排行
  - `GasOracleResponse`: 慢/平/快三档优先费与基础费建议
  - `NetworkStatsResponse`: 实时 TPS、平均出块时间、Gas 饱和度等
  - `Vec<TransactionDetailResponse>`: 巨鲸转账列表
  - `TopContractsResponse`: 热门合约列表（调用量、独立用户量、总 Gas）
  - `Vec<TransactionDetailResponse>`: 失败交易列表

## Acceptance Criteria
- [x] 验收标准 1：`get_block_gas_consumers` 准确按 `from_address` 汇总区块内各用户总消耗的 Gas（`gas_used`）及交易笔数，并按降序排列。
- [x] 验收标准 2：`get_gas_oracle` 基于最近区块的分位数（20th, 50th, 80th）返回准确的慢、正常、快速三档费率及最新 Base Fee。
- [x] 验收标准 3：`get_network_stats` 计算并返回最新高度、时间戳、最近 1 小时 TPS、平均出块耗时与 Gas 饱和度。
- [x] 验收标准 4：`get_whale_transfers` 利用 `toUInt256OrZero(value)` 筛选大额转账并支持游标分页。
- [x] 验收标准 5：`get_top_contracts` 统计指定窗口内合约调用量、独立用户数与 Gas 消耗并降序返回。
- [x] 验收标准 6：`get_failed_transactions` 准确返回 `status = 0` 的交易并支持分页。
- [x] 验收标准 7：全部单元测试与集成测试通过，编译无警告。

## Verification Commands
```bash
cargo test --ignore-rust-version --test block_transaction_test
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true cargo test --ignore-rust-version --test clickhouse_integration_tests
cargo test --ignore-rust-version
cargo check --ignore-rust-version --all-targets
```

## Risks and Assumptions
- 风险：极低，仅在只读查询层新增函数与路由，不影响任何后台写入流水线。
- 假设：ClickHouse 表结构维持原有定义。

## Status
DONE

