# TASK-019: 实现 RPC 节点池真实平滑加权轮询 (Smooth Weighted Round-Robin) 负载均衡并保留故障熔断与冷却自愈

## Objective
将 RPC 节点池的调度选择逻辑从“静态主备优先级（只取排在第一位的高权重节点）”重构为标准的“平滑加权轮询（Smooth Weighted Round-Robin, SWRR）”算法，使得同链的多个可用 RPC 节点根据其配置的 `weight` 比例平滑、交替地均摊请求流量，同时保留现有完善的错误熔断、阶梯冷却、探活自愈与全节点故障 fallback 机制。

## Scope
- 包含：
  - 在 `src/rpc_pool/mod.rs` 的 `EndpointRuntimeStatus` 中增加 `current_weight: i32`，用于记录 SWRR 算法运行时动态权重；
  - 实现纯函数及内部调度的 `select_weighted_round_robin` 算法，确保无节点饥饿、平滑交织（如权重 4:1 产生 A, A, B, A, A 的平滑调度序列）；
  - 在 `select_rpc_endpoint` 中使用 SWRR 调度健康可用节点（及非冷却节点）；
  - 在 `tests/rpc_pool_cooldown_test.rs` 中补齐加权轮询调度比例、交替均匀度、故障节点被排除后的动态重分配测试；
  - 维护 `docs/AI/TASK_INDEX.md` 与 `docs/AI/SESSION_STATE.md`。
- 不包含：
  - 修改数据库 DDL（`eventlake_rpc_endpoints.weight` 保持现有的 `i32` 整数，无需变动 schema）；
  - 修改外部 REST API 接口；
  - 引入任何笨重的外部随机数或调度 crate（依托现有 `ENDPOINT_RUNTIME_STATES` 内存状态）。

## Allowed Files
- `src/rpc_pool/mod.rs`
- `tests/rpc_pool_cooldown_test.rs`
- `docs/AI/tasks/TASK-019.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-018 (`DONE`)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 同一 `chain_id` 下的多条启用的 `RpcEndpointRecord`（含各自的 `weight` 和 `status`）。
- **Outputs**: 按照 `weight` 比例平滑分发的 `RpcEndpointRecord` 实例，保证多节点并发均摊流量。

## Acceptance Criteria
- [x] 标准 1：在多节点可用时，`select_rpc_endpoint` 不再永远返回第一位的高权重节点，而是按权重比例交替返回所有可用节点。
- [x] 标准 2：权重相同时（如 100:100），实现完美的 1:1 交替轮询（A, B, A, B）。
- [x] 标准 3：权重不同时（如 4:1），实现严格且平滑的 4:1 交织调度（如 5 次请求中 A 获得 4 次，B 获得 1 次，且均匀穿插）。
- [x] 标准 4：当某节点调用失败进入 Cooldown 冷却期后，SWRR 候选集动态剔除该节点，剩余健康节点按各自权重平滑分摊流量；节点探活自愈后平滑重新接入轮换。
- [x] 标准 5：全库测试（含新增加权轮询测试）`cargo test --ignore-rust-version` 100% 通过（60/60 全部通过）。

## Verification Commands
```bash
# 1. 运行 RPC 节点池测试套件
cargo test --ignore-rust-version --test rpc_pool_cooldown_test -- --nocapture

# 2. 运行全库完整测试套件
cargo test --ignore-rust-version

# 3. 静态代码检查
cargo check --ignore-rust-version --all-targets
```

## Risks and Assumptions
- 风险：并发请求多线程访问 `ENDPOINT_RUNTIME_STATES` 时需保证写锁操作极快，避免线程竞争。SWRR 算法为 O(N)（N 为候选节点数，通常单链不超过 5 个），耗时小于 1 微秒，开销可忽略。
- 假设：`weight` 均为大于 0 的正整数（已有接口参数校验 `weight > 0` 保证）。

## Status
DONE
