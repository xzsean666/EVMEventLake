# TASK-027: 区块与交易多节点并发分片抓取流水线与切片故障自愈顶替机制

## Objective
实现区块与交易同步在历史追赶阶段的多节点并发分片（Multi-Node Chunk Pipeline）流水线，提升历史同步吞吐量。同时引入单切片故障自动顶替机制（Failover Takeover），确保当某个节点发生临时网络抖动或超限报错时，由节点池中其他健康节点自动接替该切片的抓取。严格遵循全量完备性约束（All-or-Nothing Completeness），只有当前批次所有切片的区块全部抓取落库后才原子推进 Checkpoint，杜绝区块空洞与漏块。

## Scope
- 包含：
  - 区块交易同步历史与实时分流：实时跟进保持单节点单切片，历史追赶启用多切片并发。
  - 节点故障动态顶替机制：单切片执行失败时自动记录节点故障（进入退避冷却）并由其他可用节点接替重试。
  - 全量完备性校验：确保当前任务包含的所有切片全部就绪并成功写入 ClickHouse 后才推进 SQLite Checkpoint。
  - 针对性单元测试与回归验证。
- 不包含：
  - 修改 ClickHouse 底层表结构。

## Allowed Files
- `src/block_transaction/collector.rs`
- `tests/block_transaction_test.rs`
- `docs/AI/tasks/TASK-027.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-026（RPC 节点能力感知与智能路由已就绪）
- 外部依赖：None

## Inputs and Outputs
- **Inputs**:
  - `BlockTransactionSyncStateRecord`: `chain_id`, `next_block`, `batch_size`, `safe_head`, `status` 等。
  - 多节点 RPC 候选池。
- **Outputs**:
  - 高并发、具备自愈顶替能力的多切片区块交易抓取与 ClickHouse 批量落盘。

## Acceptance Criteria
- [x] 历史追赶状态下，支持将连续区块划分为多个并发切片向不同 RPC 节点分发请求。
- [x] 当某个 RPC 节点拉取切片失败时，自动将该节点置入冷却并由其他可用 RPC 节点顶替重试，直至切片成功。
- [x] 任务内所有切片全部落库成功后，Checkpoint 才向前推进；若最终有切片不可恢复，整批失败且不推进水位，杜绝数据空洞。
- [x] 实时跟进状态下保持单节点顺序同步与 Reorg 安全检测。
- [x] 单元与集成测试全部通过。

## Verification Commands
```bash
cargo test --ignore-rust-version --test block_transaction_test
cargo check --ignore-rust-version --all-targets
```

## Risks and Assumptions
- 风险：极低，ClickHouse `blocks` 与 `transactions` 表具有去重主键，并发批量写入幂等安全。
- 假设：RPC 节点池具备至少一个可用健康节点。

## Status
DONE

