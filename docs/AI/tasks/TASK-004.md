# TASK-004: 实现 RPC 节点池阶梯式退避冷却 (1m->5m->24h) 与内存化路由缓存

## Objective
针对用户多 RPC 节点的生产环境，在 RPC 节点池中实现动态错误阶梯式退避冷却（CD）机制（失败 1 次冷却 1 分钟，连续失败 2 次冷却 5 分钟，随后阶梯递增直至最大 24 小时冷却；只要节点调用成功则清空 CD 并恢复健康状态）；同时结合内存路由缓存，避免对故障节点的无效请求重试，大幅提升 RPC 调度效率与系统韧性。

## Scope
- 包含：
  - 在 `src/rpc_pool/` 中升级节点健康状态追踪模型：
    - 记录 `consecutive_failures: u32`、`cooldown_until: Option<DateTime<Utc>>`。
    - 实现阶梯退避时长计算：
      - 失败 1 次：1 分钟 (60s)
      - 失败 2 次：5 分钟 (300s)
      - 失败 3 次：15 分钟 (900s)
      - 失败 4 次：1 小时 (3600s)
      - 失败 5 次：4 小时 (14400s)
      - 失败 6 次及以上：24 小时上限 (86400s)
    - 实现调用成功后的 CD 重置：`consecutive_failures = 0`, `cooldown_until = None`。
  - 优化节点路由选择策略：
    - 优先从健康（非 CD 状态）节点列表中按权重/轮询选取。
    - 当某链所有可用节点均在 CD 状态时，降级选择 CD 剩余时间最短的节点兜底并输出 Warning 日志。
  - 增加内存快速状态查询与管理 API：支持通过 API 查询各节点当前冷却状态、剩余 CD 秒数与失败历史。
  - 编写详尽的单元测试，验证阶梯退避、上限封顶、成功复位及全节点 CD 降级兜底行为。
- 不包含：
  - 采集流水线并发化（由 TASK-005 处理）。

## Allowed Files
- `src/rpc_pool/mod.rs`
- `src/rpc_pool/evm_rpc_client.rs`
- `tests/rpc_pool_cooldown_test.rs`
- `docs/AI/tasks/TASK-004.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-003
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 现有的 RPC 端点配置与 `RpcPoolState`。
- **Outputs**: 具备阶梯退避冷却与快速自愈能力的 RPC 连接池管理器及配套测试用例。

## Acceptance Criteria
- [x] 节点连续失败时，冷却时间按 1m -> 5m -> 15m -> 1h -> 4h -> 24h 阶梯递增，最大不超过 24 小时。
- [x] 节点调用成功时，连续失败计数重置为 0，冷却时间立即清空。
- [x] 路由选择自动避开处于 CD 期的节点；若全在 CD，平滑降级至 CD 剩余时间最小的节点。
- [x] 新增 `tests/rpc_pool_cooldown_test.rs` 测试套件并通过。
- [x] `cargo check --ignore-rust-version --all-targets` 检查无错误。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version --test rpc_pool_cooldown_test
cargo test --ignore-rust-version --features clickhouse --lib
```

## Risks and Assumptions
- 风险：若所有配置的节点由于网络断开同时进入冷却，需要降级选择 CD 最小节点以允许自愈探测，不能彻底死锁。
- 假设：时间比对采用标准系统时钟 `Utc::now()`。

## Status
DONE
