# TASK-005: 核心日志采集流水线并发化改造与多链/多订阅异步隔离

## Objective
重构 `src/collector/worker.rs` 串行处理循环，引入基于 `tokio::task::JoinSet` 与轻量信号量（`tokio::sync::Semaphore`）的并发采集流水线，支持跨链及独立订阅桶并行拉取 RPC 与批量提交存储，成倍提升吞吐量与历史数据追赶效率，同时保证同链同合约区间进度更新的原子性与严格递增。

## Scope
- 包含：
  - 在配置中引入可配置采集并发上限（如 `collector_concurrency: usize`，默认 4~8）。
  - 在 `src/collector/worker.rs` 中将 `bucket_subscriptions` 后的各 bucket 调度改造为有界并发执行（JoinSet / BufferUnordered）。
  - 对同链同订阅的状态更新与 Reorg 检查维持链内顺序保障，避免并发竞争导致块高回退或错乱。
  - 采集失败时的局部隔离：单个 bucket 发生 RPC 错误或进入 CD 时，不阻塞其他链或其他 bucket 的正常同步流水线。
  - 编写并发采集与错误隔离测试用例。
- 不包含：
  - 存储模式重大迁移。

## Allowed Files
- `src/collector/worker.rs`
- `src/collector/mod.rs`
- `src/configuration/mod.rs`
- `tests/collector_concurrency_tests.rs`
- `docs/AI/tasks/TASK-005.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-004
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 订阅配置与 RPC 连接池。
- **Outputs**: 高并发、高吞吐且具备故障链局部隔离的生产级采集工作进程。

## Acceptance Criteria
- [x] 支持多订阅桶的有界并发采集，并发度可配置（或按 CPU/连接池动态适配）。
- [x] 某链某订阅发生 RPC 失败时不阻塞其他链或独立桶的同步进度。
- [x] 同一订阅进度的检查与游标持久化保持严格幂等与顺序安全。
- [x] 单元/集成测试验证并发正确性与错误隔离。
- [x] `cargo check --ignore-rust-version --all-targets` 检查无错误。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version --test collector_concurrency_tests
cargo test --ignore-rust-version --features clickhouse --lib
```

## Risks and Assumptions
- 风险：高并发可能触碰下游 RPC 限流，需要配合 TASK-004 的 CD 冷却机制与滑动窗口退避。
- 假设：数据库写连接池能够承受并发批量写入。

## Status
DONE
