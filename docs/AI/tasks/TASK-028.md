# TASK-028: 基于可用 RPC 动态并发与节点能力自适应切片流水线

## Objective
优化区块与交易收集流水线：
1. 解除区块与交易收集对 `needs_archive: true` 的强依赖，使普通全节点与归档节点共同参与历史区块与交易的高并发拉取。
2. 将并发切片数从固定静态数值升级为**基于当前链可用健康 RPC 节点数量动态确定**。
3. 引入**节点能力自适应切片（Capacity-Aware Chunk Slicing）**：根据分配给各个节点的 `max_batch_size` 差异化切分连续区块区间（例如官方节点切 50 块、Sequence 切 20 块、dRPC 切 10 块），让每个节点均在最优吞吐点工作。
4. 保留并增强单切片故障动态顶替（Failover Takeover）与全量完备性（All-or-Nothing）校验，杜绝漏块与空洞。

## Scope
- 包含：
  - 区块交易收集路由要求调整：`needs_archive: false`，释放普通全节点并发算力。
  - 动态并发度计算：获取当前链所有健康可用节点列表，以可用节点数决定并发切片数。
  - 节点能力自适应切片算法：按节点 `max_batch_size` 依次切分区间。
  - 切片流水线派发与故障顶替自愈。
  - 针对性单元测试与回归验证。
- 不包含：
  - 合约 Logs 收集逻辑调整（已在 TASK-026 中支持动态安全切片）。

## Allowed Files
- `src/rpc_pool/mod.rs`
- `src/block_transaction/collector.rs`
- `tests/block_transaction_test.rs`
- `docs/AI/tasks/TASK-028.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-027（已完成基础切片并发与全量落库校验）
- 外部依赖：None

## Inputs and Outputs
- **Inputs**:
  - `BlockTransactionSyncStateRecord`: `chain_id`, `next_block`, `batch_size`, `safe_head`, `status` 等。
  - 当前链所有可用健康 RPC 节点及其能力参数（`max_batch_size`, `weight`）。
- **Outputs**:
  - 按节点吞吐能力定制的多切片高并发区块交易抓取流水线与 ClickHouse 批量落盘。

## Acceptance Criteria
- [x] 区块与交易抓取解除 `needs_archive: true` 强限制，普通全节点与 Archive 节点共同参与拉取。
- [x] 并发切片数基于当前链可用健康 RPC 数量动态确定，不盲目超发也不闲置可用节点。
- [x] 切片大小基于各个被分配节点的 `max_batch_size` 差异化自适应切分，使各节点在最高效参数下工作。
- [x] 单切片具备故障自愈顶替能力（Failover Takeover），且满足全量完备性（All-or-Nothing）与全局连续性校验。
- [x] 针对性单元测试与全目标编译通过。

## Verification Commands
```bash
cargo test --ignore-rust-version --test block_transaction_test
cargo check --ignore-rust-version --all-targets
```

## Risks and Assumptions
- 风险：极低，ClickHouse 写入幂等，全量成功后推进 Checkpoint 保证强一致性。
- 假设：配置中至少存在 1 个可用的 RPC 节点。

## Status
DONE

