# 动态多地址聚合拼车采集（Dynamic Multi-Address Carpool Collection）任务拆解

版本：1.0  
状态：待实现（Step 4 任务清单）  
适用仓库：`EVMEventLake`  

---

## 任务概览

| 序号 | 任务模块 | 文件位置 | 核心变动内容 |
| :--- | :--- | :--- | :--- |
| **Task 1** | 运行时配置扩展 | `src/configuration/mod.rs` | 增加 `max_batch_addresses` 参数（默认 50），支持环境变量 `EVENTLAKE_COLLECTOR_MAX_BATCH_ADDRESSES` |
| **Task 2** | RPC 客户端多地址支持 | `src/rpc_pool/evm_rpc_client.rs` | 改造 `eth_get_logs` 参数为 `contract_addresses: &[String]`，单地址序列化为 String，多地址序列化为 Array |
| **Task 3** | 批量分桶调度与日志分发 | `src/collector/worker.rs` | 引入基于 `(chain_id, current_block, status)` 的动态分桶算法，对同高度地址批量发起请求，并通过 `log.address` 进行 Log Demuxing |
| **Task 4** | 订阅检查点批量更新 | `src/subscriptions/mod.rs` | 提供批次更新订阅进度与状态的 SQL 辅助方法 `update_checkpoints_batch` |
| **Task 5** | 单元与集成测试验证 | `tests/` | 增加多地址合并请求、日志归属分发、并发进度推进及错误降级的完整测试用例 |

---

## 详细实施计划

### Task 1: 运行时配置扩展 (`src/configuration/mod.rs`)
- 在 `BackgroundConfiguration` 中增加 `pub max_batch_addresses: usize`。
- 在 `Environment` 解析中支持 `EVENTLAKE_COLLECTOR_MAX_BATCH_ADDRESSES`，默认值为 50。
- 校验：必须 `>= 1` 且 `<= 500`。

### Task 2: RPC 客户端扩展 (`src/rpc_pool/evm_rpc_client.rs`)
- 改造 `pub async fn eth_get_logs`:
  ```rust
  pub async fn eth_get_logs(
      client: &Client,
      rpc_url: &str,
      contract_addresses: &[String],
      from_block: i64,
      to_block: i64,
  ) -> Result<Vec<RpcLog>, ApplicationError>
  ```
- 行为：
  - `contract_addresses.is_empty()`：不添加 `address` 过滤项（供 `all_events` 使用）。
  - `contract_addresses.len() == 1`：`"address": "0x..."`。
  - `contract_addresses.len() > 1`：`"address": ["0x...", "0x..."]`。
- 编写该函数的单元测试覆盖单地址与多地址序列化。

### Task 3: 采集器分桶与分发 (`src/collector/worker.rs`)
- 将 `subscriptions::runnable_subscriptions` 调度的单次拉取量由 10 调整为按需动态拉取（如 50~100）。
- 分组算法：
  - 过滤出 `collection_scope == "contract"` 的订阅。
  - 按 `(chain_id, current_block, status)` 归类。
  - 对于同一组内地址超过 `max_batch_addresses` 的，调用 `.chunks(max_batch_addresses)` 切片。
- 执行流程：
  - 取切片内所有订阅的最小窗口 `window = chunk.iter().map(|s| s.current_block_window).min().unwrap()`。
  - 发送多地址 `eth_get_logs`。
  - 构造 `HashMap<normalized_address, SubscriptionRecord>`。
  - 遍历返回的 logs，通过 `log.address` 精准关联对应的订阅并存入存储层。
  - 批量推进该批订阅检查点至 `to_block + 1`。
- 容错策略：
  - 批次若触发 `is_get_logs_window_error`，批次内所有订阅同步减半窗口；
  - 若多次重试仍失败，自动拆分为单个订阅分别执行。

### Task 4: 数据库批量更新 (`src/subscriptions/mod.rs`)
- 编写 `update_checkpoints_batch` 函数，使用 `QueryBuilder` 构造单条高效 `UPDATE ... FROM (VALUES ...)` 批量更新订阅检查点，减少数据库交互往返。

### Task 5: 测试用例覆盖
- 验证 mock RPC 接收到的 JSON-RPC 请求格式是否符合标准。
- 验证不同合约的事件被准确写入对应的 `subscription_id`。
- 验证当有新地址在不同高度时，该新地址独自追赶，其余同高度地址保持合并。
