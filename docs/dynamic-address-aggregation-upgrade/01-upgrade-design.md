# 动态多地址聚合拼车采集（Dynamic Multi-Address Carpool Collection）升级设计

版本：1.0  
状态：待实现（Step 1 & Step 2 已就绪，等待 Step 4 代码审批）  
适用仓库：`EVMEventLake`  
目标实现者：Antigravity Agent  

---

## 1. 背景与目标

### 1.1 现状与性能瓶颈
在当前的 EventLake 架构中：
* 用户已可通过 `/api/subscriptions/batch` 一次性提交上千个合约地址，生成一批 `contract`-scoped 订阅。
* 但后台采集器（[`src/collector/worker.rs`](file:///home/sean/git/EVMEventLake/src/collector/worker.rs)）每次只拉取 10 个订阅，且对每个订阅**按单地址分别发起一次独立的 `eth_getLogs` 请求**：
  ```rust
  eth_get_logs(client, url, subscription.contract_address.as_deref(), from, to)
  ```
* **瓶颈表现**：
  1. **RPC 配额与 RTT 浪费**：若订阅了 100 个代币合约，在实时监控模式（`realtime_syncing`）下，每个区块都需要向 RPC 节点发送 100 次单独的 HTTP 请求，严重浪费 RPC 配额并受网络延迟制约。
  2. **同步延迟积压**：当订阅量增长至数百或上千时，单地址轮询无法及时追上链上最新区块，导致监控数据产生延迟滞后。

### 1.2 升级目标
1. **多地址动态拼车（Carpooling / Batch Aggregation）**：
   在同链（`chain_id` 一致）、同进度（`current_block` 相同）的订阅之间，动态聚合为批次（Batch），单次 `eth_getLogs` 传入 `address: ["0x1...", "0x2..."]`。
2. **零耦合与独立生命周期**：
   数据库中不进行物理绑死（无静态组概念）。每个地址依然是独立的 `SubscriptionRecord`，拥有独立的 Checkpoint，任意地址的新增、暂停、恢复、历史追赶不影响全局批次。
3. **分批容量与防打爆机制（Chunking）**：
   引入配置项 `max_batch_addresses`（默认 50~100），防止超出 RPC 节点的地址数组上限与结果返回上限。
4. **日志精准分发（Log Demuxing）**：
   批量返回的日志流通过 `log.address` 精准映射回对应的 `subscription_id`，保证落库（PostgreSQL 或 ClickHouse）与检查点完全可追溯。
5. **故障降级与木桶效应隔离**：
   若合并请求遇到 RPC 限制或异常，具备自动缩容窗口或降级拆分为单地址重试能力，避免一个异常合约阻碍同批所有合约。

---

## 2. 架构设计与数据流

### 2.1 整体架构流程

```text
               PostgreSQL eventlake_subscriptions
                             │
                             ▼
              subscriptions::runnable_subscriptions (获取待执行任务)
                             │
                             ▼
      ┌──────────────────────────────────────────────┐
      │          Collector: 动态分桶与切片             │
      │  按 (chain_id, current_block, status) 分桶   │
      │  每桶按 max_batch_addresses (如 50) 切成 Chunk │
      └──────────────────────────────────────────────┘
                             │
           ┌─────────────────┴─────────────────┐
           ▼                                   ▼
    [单地址 / 单独历史批]                 [多地址聚合拼车批]
           │                                   │
           │                     eth_getLogs(address: [A, B, C...], from, to)
           │                                   │
           ▼                                   ▼
     RPC 节点返回 Logs                  RPC 节点返回 Logs
           │                                   │
           │                     ┌─────────────┴─────────────┐
           │                     │  Log Demuxing (分发器)    │
           │                     │  按 log.address 匹配 SubId │
           │                     └─────────────┬─────────────┘
           │                                   │
           ▼                                   ▼
  ┌──────────────────────────────────────────────────────────┐
  │ 存储层落库: eventlake_raw_logs (PG) 或 raw_logs (ClickHouse)│
  └──────────────────────────────────────────────────────────┘
                             │
                             ▼
  ┌──────────────────────────────────────────────────────────┐
  │ 检查点原子推进: 批量更新该批订阅 current_block = to_block + 1 │
  └──────────────────────────────────────────────────────────┘
```

### 2.2 核心工作阶段划分

#### 阶段 A：历史追赶阶段（Historical Sync）
* 新增地址通过 API 注册后，初始状态为 `pending`，享有最高调度优先级。
* 若新增地址与其他地址高度不同，它保持单地址（或与同一 batch 导入且起始高度相同的地址）独立追赶历史，窗口根据其自身的日志密度自适应缩放（`shrink_block_window` / `grow_block_window`）。
* 追上最新安全区块高度（`safe_head`）后，自动转换为 `realtime_syncing` 状态。

#### 阶段 B：实时监控阶段（Realtime Sync & Carpool）
* 已经追平安全高度的地址，其 `current_block` 处于相同的刻度。
* 调度器在内存中以 `(chain_id, current_block)` 作为 Hash Key 自动将它们归入同一个 Bucket。
* 每个 Bucket 取出前 `N` 个地址（`max_batch_addresses`，例如 50 个）组成一个聚合批次。
* 统一计算安全区间 `[from_block, to_block]`，发起单次多地址 `eth_getLogs`。

---

## 3. 详细设计与模块改动

### 3.1 运行配置（`src/configuration/mod.rs`）
在后台配置中增加批次上限参数：
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct BackgroundConfiguration {
    pub worker_tick: Duration,
    pub max_batch_addresses: usize, // 新增：单次 eth_getLogs 允许合并的最大地址数，默认 50
    // ...
}
```
默认环境变量：`EVENTLAKE_COLLECTOR_MAX_BATCH_ADDRESSES=50`。

### 3.2 RPC 客户端支持多地址（`src/rpc_pool/evm_rpc_client.rs`）
扩展 `eth_get_logs` 接口：
```rust
pub async fn eth_get_logs(
    client: &Client,
    rpc_url: &str,
    contract_addresses: &[String], // 支持空（全量）、单地址、或多地址数组
    from_block: i64,
    to_block: i64,
) -> Result<Vec<RpcLog>, ApplicationError> {
    let mut filter = serde_json::Map::new();
    if contract_addresses.len() == 1 {
        filter.insert("address".to_owned(), json!(normalize_hex(&contract_addresses[0])));
    } else if contract_addresses.len() > 1 {
        let normalized_addresses: Vec<String> = contract_addresses
            .iter()
            .map(|a| normalize_hex(a))
            .collect();
        filter.insert("address".to_owned(), json!(normalized_addresses));
    }
    filter.insert("fromBlock".to_owned(), json!(format!("0x{:x}", from_block)));
    filter.insert("toBlock".to_owned(), json!(format!("0x{:x}", to_block)));
    let params = json!([filter]);

    call(client, rpc_url, "eth_getLogs", params).await
}
```

### 3.3 采集器动态分桶与调度（`src/collector/worker.rs`）
重构 `collect_once` 逻辑：
1. **拉取更多候选订阅**：将查询批次从固定 10 扩大（如 100~200 条活跃订阅）。
2. **多地址分组分桶**：
   * 将 `collection_scope == "all_events"` 的订阅单独处理（无地址过滤）。
   * 将 `contract` 订阅按照 `(chain_id, current_block, status)` 进行分组。
   * 对于同高度、同链的分组，切片为大小不大于 `max_batch_addresses` 的批次。
3. **批量采集执行**：
   * 批次内取最小窗口：`window = min(subscription.current_block_window)`。
   * 执行 `eth_get_logs(client, url, &addresses, from, to)`。
   * **Log Demuxing**：建立 `address -> SubscriptionRecord` 快速哈希查找表，逐条分配日志的 `subscription_id`。
   * **存储与检查点**：批量写入日志存储，随后在事务中批量更新该批订阅的 `current_block`。
4. **异常降级重试**：
   * 若多地址请求报错（例如日志过多），优先对批次内各订阅同步缩小窗口；若依然失败，则拆分为单个订阅独立采集，确保故障隔离。

---

## 4. 容错与边界情况处理

| 场景 | 行为与处理策略 |
| :--- | :--- |
| **新增地址起步高度不同** | 该地址单独进行历史追补，直至其 `current_block` 赶上 `safe_head` 并自动归入实时拼车分桶。 |
| **单批地址日志数量超标** | 触发 `is_get_logs_window_error`，窗口折半（`shrink_block_window`）。若窗口已到最小值仍报错，自动拆分为单地址重试。 |
| **链发生 Reorg** | 区块 Hash 校验（`observe_block`）失败时，触发该高度区间的回退与失效，涉及到的所有订阅原子回退游标并重新拉取。 |
| **部分地址被暂停/删除** | 数据库中 `active=false` 或 `status='paused'`，调度器不再拉取该记录，后续分桶自然将其剔除，其他地址继续拼车。 |
| **RPC 节点不支持数组 address** | 极个别非标准节点限制可通过配置 `max_batch_addresses=1` 优雅回退为完全单地址模式。 |

---

## 5. 测试与验证计划

1. **单元测试**：
   - 验证 `eth_get_logs` 生成的 JSON 请求体：单地址生成字符串 `"address": "0x..."`，多地址生成数组 `"address": ["0x...", "0x..."]`。
   - 验证分桶算法（按 chain_id, current_block 分流，超额自动分 chunk）。
2. **集成测试**：
   - 多合约批量订阅后，验证在相同高度下是否只发出了单次合并 RPC 请求。
   - 验证返回的不同合约 Log 能准确绑定各自的 `subscription_id`。
   - 验证批次中各订阅的 `current_block` 同步推进。
   - 验证异常场景下的降级行为与重试机制。
