# TASK-026: RPC 节点能力感知、Archive/普通节点分流调度与合约 Logs 动态切片策略

## Objective
实现基于 RPC 节点能力（Archive 属性、最大区块跨度、最大 Batch 大小）的精细化治理与智能路由调度。支持历史追赶与实时同步差异化路由（非 Archive 节点分流实时高频流量，Archive 节点专属服务历史深度追赶），并在合约日志采集端引入基于节点能力的动态安全切片（Dynamic Clamping），允许冷门合约以超大跨度（最高达百万级区块）安全高速追赶，彻底消除不同节点能力不一带来的木桶短板限制。

## Scope
- 包含：
  - 压平基础 Schema：直接在 `migrations/202609180001_initial_schema.sql` 中的 `eventlake_rpc_endpoints` 增加 `is_archive`, `max_block_range`, `max_batch_size` 字段（无需递增迁移文件）。
  - RPC Pool 核心增强：升级 `RpcEndpointRecord`，引入 `EndpointRequirements` 与 `select_rpc_endpoint_with_requirements`，支持基于 Archive 需求和推荐区块跨度的智能平滑加权轮询（SWRR）。
  - 合约日志收集器增强：识别 `historical_syncing` 状态，传递 `needs_archive` 路由需求；在请求前依据节点 `max_block_range` 实施动态安全切片（Dynamic Clamping），杜绝超限报错与无谓降级。
  - 区块交易同步适配：依据节点 `max_batch_size` 自动安全截断批处理大小。
  - 配置文件与测试用例同步。
- 不包含：
  - 保留历史旧版本迁移文件或废弃兼容层。

## Allowed Files
- `migrations/202609180001_initial_schema.sql`
- `src/rpc_pool/mod.rs`
- `src/collector/worker.rs`
- `src/block_transaction/collector.rs`
- `config/rpc_endpoints.json`
- `config/rpc_endpoints.json.example`
- `config/eventlake.example.json`
- `tests/rpc_pool_cooldown_test.rs`
- `docs/AI/tasks/TASK-026.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-025 (Soneium 节点预置已完成)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**:
  - `eventlake_rpc_endpoints` 新增能力配置列：`is_archive` (bool), `max_block_range` (i64), `max_batch_size` (i32)。
  - `EndpointRequirements`: 包含 `needs_archive` 与 `preferred_block_range`。
- **Outputs**:
  - 智能路由与动态切片后的高效、零超限日志采集与区块同步。

## Acceptance Criteria
- [x] SQLite 迁移自动升级 `eventlake_rpc_endpoints`，默认字段向后兼容。
- [x] RPC 节点池在 `needs_archive = true` 时仅调度 `is_archive = true` 节点；在 `needs_archive = false` 时混合调度 Archive 与普通节点。
- [x] 日志采集器对配置了 `max_block_range` 的节点自动切片，合约即使配置了 1,000,000 块跨度也不会在小跨度节点报错。
- [x] 单元与集成测试全部通过。

## Verification Commands
```bash
cargo test --ignore-rust-version --test rpc_pool_cooldown_test
cargo test --ignore-rust-version --test collector_concurrency_tests
cargo test --ignore-rust-version --test validation_tests
```

## Risks and Assumptions
- 风险：极低，旧节点无标注时默认 `is_archive = true, max_block_range = None`，保证平滑兼容。
- 假设：SQLite 支持在已有表中新增默认列。

## Status
DONE
