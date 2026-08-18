# Block / Transaction 数据采集与 API 升级设计

版本：1.0  
状态：待实现  
适用仓库：`EVMEventLake`  
目标实现者：Gemini 3.7 Flash

## 1. 背景与目标

当前仓库已经具备完整的 raw log 收集模块（现有 `src/collector`）、PostgreSQL 运行状态管理和可选 ClickHouse
存储，但还没有完整的 block / transaction 数据集。现有文档
[`EVM Block & Transaction Data Collection Architecture`](../evm-block-transaction-data-collection-architecture.md)
规定了数据来源：通过 `eth_getBlockByNumber(blockNumber, true)` 获取区块和区块内的
完整交易。

本次升级要交付：

1. 一个可断点恢复、可重试、可检测 reorg 的 block / transaction collector。
2. 一个 ClickHouse 数据面，保存 canonical block 和 transaction 行。
3. 一组只依赖已采集字段的数据 API，支持区块查询、交易查询和地址交易列表。
4. OpenAPI、迁移、测试、配置和运维文档，使功能可部署、可验证、可回滚。

本次升级不实现 ABI 解码，也不声称提供未采集数据。

**重要边界：log 收集模块已经存在并继续作为现有能力运行。本任务不新增、不替换、不
重写 log collector，也不改变 raw-log 的采集、checkpoint、reorg 和 API 语义；只新增
独立的 block/transaction collector。**

## 2. 与当前架构的边界

### 2.1 数据职责

| 职责 | PostgreSQL | ClickHouse |
| --- | --- | --- |
| chain、RPC endpoint、认证 | 保留 | 不写入 |
| block/transaction checkpoint | 保留 | 不写入 |
| block、transaction 原始结构化数据 | 不写入 | 主数据面 |
| raw logs | 按现有配置 | 按现有配置 |
| ABI 解码、receipt、trace | 不新增 | 不新增 |

block/transaction 数据量和查询模式不适合复用 `eventlake_raw_logs`。新增功能在
ClickHouse 中使用独立的 `blocks` 与 `transactions` 表；不得把 block/transaction
行塞进 raw log 表，也不得让 ClickHouse 故障时静默回退到 PostgreSQL。

建议新增运行开关 `EVENTLAKE_BLOCK_TRANSACTION_ENABLED`。开关为 `true` 时，二进制必须
带 `clickhouse` feature 且 ClickHouse 可用；否则启动失败或 API 返回明确的
`503 block transaction storage unavailable`。默认关闭，确保现有部署行为不变。

### 2.2 采集来源

主请求：

```text
eth_getBlockByNumber("0x<block>", true)
```

`true` 是必要条件，否则只能得到 transaction hash。批量请求使用 JSON-RPC batch，
每个请求必须有可关联的 `id`，响应按 `id` 匹配，不能依赖响应顺序。单个 batch
失败时允许拆成单块重试；不能因为一个块失败而推进整个范围的 checkpoint。

不调用以下方法作为本阶段必需依赖：`eth_getTransactionReceipt`、`eth_getLogs`、
`debug_traceTransaction`、`trace_block`。receipt、logs、traces 属于后续独立模块。

### 2.3 与现有 raw-log collector 的关系

现有 raw-log collector 已经负责 `eth_getLogs` 和 log 数据落库。新增 block/transaction
collector 必须放在独立模块（建议 `src/block_transaction/`）和独立 worker 中；两者只
共用 RPC endpoint 选择、HTTP client、链配置和 tracing 风格，但有
各自的 checkpoint、错误状态和存储写入函数。不要复用 subscription checkpoint：
subscription checkpoint 表示某个 log 过滤器的进度，不能代表整条链的 block 进度。

除非为了提取一个有回归测试保护的通用 helper，否则不要修改 `src/collector/worker.rs`
或现有 log collector。新 worker 的注册也应保持现有 log worker 的行为不变。

## 3. 数据模型

### 3.1 Blocks

ClickHouse `blocks` 一行表示一个 `(chain_id, block_number)` 的最新版本。至少保存：

| 字段 | 语义 | API 编码 |
| --- | --- | --- |
| `chain_id` | EVM chain id | JSON number |
| `block_number` | 区块高度 | JSON number |
| `block_hash` | 区块 hash | `0x` + 64 位小写 hex |
| `parent_hash` | 父区块 hash | 同上 |
| `timestamp` | RPC 返回的 Unix seconds | JSON number |
| `gas_limit` | 区块 gas limit | 十进制字符串 |
| `gas_used` | 区块 gas used | 十进制字符串 |
| `base_fee_per_gas` | EIP-1559 base fee，可为空 | 十进制字符串或 null |
| `beneficiary` | `miner` / `author` | 42 字符小写地址或 null |
| `transactions_root` | 交易树根 | hash 或 null |
| `receipts_root` | receipt 树根 | hash 或 null |
| `state_root` | state 树根 | hash 或 null |
| `size` | RPC block size，可为空 | 十进制字符串或 null |
| `withdrawals_root` | Shanghai 后字段 | hash 或 null |
| `blob_gas_used` | Cancun 后字段 | 十进制字符串或 null |
| `excess_blob_gas` | Cancun 后字段 | 十进制字符串或 null |
| `parent_beacon_block_root` | Cancun 后字段 | hash 或 null |
| `transaction_count` | `transactions` 数组长度 | JSON number |
| `is_canonical` | 当前是否 canonical | API 只返回 `true` 行 |
| `stored_at` | 数据版本时间 | API 可选，不作为链上时间 |

RPC 字段缺失在不同客户端和 fork 上是正常情况，应落为 `NULL`，不能把 `0` 当成缺失。
`miner` 和 `author` 兼容处理：优先 `miner`，没有时读取 `author`。

### 3.2 Transactions

ClickHouse `transactions` 一行表示一笔交易。至少保存：

| 字段 | 语义 | API 编码 |
| --- | --- | --- |
| `chain_id` | EVM chain id | JSON number |
| `tx_hash` | 交易 hash | `0x` + 64 位小写 hex |
| `block_number` | 所属区块 | JSON number |
| `transaction_index` | 区块内序号 | JSON number |
| `from_address` | 发送方 | 42 字符小写地址 |
| `to_address` | 接收方；合约创建为 null | 地址或 null |
| `value` | 原生币数量 | 十进制字符串 |
| `nonce` | sender nonce | 十进制字符串 |
| `gas` | gas limit | 十进制字符串 |
| `gas_price` | legacy/effective gas price（仅 RPC 有值时） | 十进制字符串或 null |
| `max_fee_per_gas` | EIP-1559 字段 | 十进制字符串或 null |
| `max_priority_fee_per_gas` | EIP-1559 字段 | 十进制字符串或 null |
| `tx_type` | `type`，兼容缺失值 | JSON number 或 null |
| `method_id` | calldata 前 4 bytes；空 input 为 null | `0x` + 8 位 hex 或 null |
| `is_canonical` | 所属区块是否 canonical | API 只返回 `true` 行 |
| `stored_at` | 数据版本时间 | API 可选 |

本阶段明确不保存完整 `input` / calldata。由于没有 receipt，不提供 `status`、
`gas_used`、`effective_gas_price`、`logs`、`contract_address`（创建结果）等字段。
不允许用 RPC 二次请求补齐这些字段后悄悄改变本阶段范围。

### 3.3 数值、hash、地址规范

- 所有 hex 输入去掉前后空白，统一 `0x` 前缀和小写；hash 长度必须为 32 bytes，地址
  必须为 20 bytes。
- `uint256` 及 gas/value/fee 等数值用十进制字符串返回，避免 JavaScript number 溢出。
- 数据库层可使用 `UInt64` 保存已确认不会溢出的高度、index、timestamp；可能是
  `uint256` 的字段使用 Decimal/UInt256/String，但 API 行为必须一致。
- `to_address`、可选 EIP 字段、fork 后才出现的 block 字段保留 null 语义。

## 4. 同步与一致性设计

### 4.1 PostgreSQL checkpoint

新增独立的每链状态表，例如 `eventlake_block_transaction_sync_state`：

```text
chain_id (PK)
next_block
safe_head
latest_seen_block
status                 -- pending / syncing / caught_up / error / reorg_retrying
last_error
last_success_at
updated_at
```

初始 `next_block` 由管理 API 或配置指定，必须明确包含该区块。checkpoint 只有在整批
block 和 transaction 成功写入 ClickHouse 后才推进到 `to_block + 1`。事务顺序必须是：

```text
读取 safe head
  -> RPC batch
  -> 校验 block number/hash/parent 与请求一致
  -> 写入 ClickHouse（可重复）
  -> PostgreSQL 更新 checkpoint
```

ClickHouse 写入失败时保留旧 checkpoint，记录可诊断错误并在下一轮重试同一区间。不得
先推进 checkpoint 再异步写数据。

### 4.2 safe head 与 realtime

`safe_head = eth_blockNumber - chains.safe_confirmation_depth`，只采集到 safe head。
追平后进入 `caught_up`，按 worker tick 继续检查新 safe head。每轮对最近
`reorg_window` 个已采集块重新取 hash，默认至少覆盖最大确认深度的一小段；具体值可配置。

### 4.3 Reorg

- 通过 `parent_hash` 连续性和 PostgreSQL 观察到的 block hash 变化识别 reorg。
- 发现变化时回退到共同祖先之后的第一个高度，旧行写入新版本
  `is_canonical=false`，canonical fork 重新写入 `is_canonical=true`。
- 所有 API 查询必须使用 `FINAL`（或等价的 latest-version 读取）并过滤
  `is_canonical=true`，确保 merge 完成前也不返回旧 fork。
- reorg tombstone 或替代行写入失败时进入 `reorg_retrying`，暂停推进，不得返回混合 fork。
- 同一块重复采集、进程重启、RPC 重试必须幂等。

### 4.4 RPC 与吞吐

- 复用 `rpc_pool::select_rpc_endpoint` 和现有失败标记机制；不要在新模块里另造 endpoint
  选择器。
- batch 大小、并发数、单请求超时和最大响应字节数配置化。默认从小 batch 开始，遇到
  timeout、response too large、provider range limit 时指数退避或拆小。
- batch 响应允许部分成功：成功块可缓存，但 checkpoint 只能推进连续成功前缀。
- 为每轮记录请求块范围、RPC endpoint、batch size、耗时、返回块数、写入行数和重试次数。

## 5. ClickHouse 表与索引建议

### 5.1 `blocks`

建议使用 `ReplacingMergeTree(stored_at)`：

```text
PARTITION BY chain_id
ORDER BY (chain_id, block_number)
```

必须能通过 `FINAL` 得到同一 chain/height 的最新版本。为 `block_hash` 增加适合精确
查询的 skipping index；不要让 hash 查询退化为全表扫描。

### 5.2 `transactions`

建议使用 `ReplacingMergeTree(stored_at)`：

```text
PARTITION BY chain_id
ORDER BY (chain_id, block_number, transaction_index, tx_hash)
```

增加 `tx_hash`、`from_address`、`to_address` 的 bloom/filter skipping index。区块内交易
列表主要依赖主排序键；交易 hash 详情和地址列表必须用 `EXPLAIN` 或基准测试确认不会
无界扫描。若地址列表在目标数据量上不达标，新增按 address 排序的 projection 或独立
`transaction_address_index`，并在任务交付中说明额外存储成本。

### 5.3 数据保留与预算

Block/transaction 是长期数据，不设置自动 TTL。实际磁盘预算要在实现后用代表性链和
压缩设置测量；原架构中的 1 TB 是最低建议，不是 API 或容量保证。首次全量同步前必须
确认 ClickHouse 数据卷容量、备份和磁盘告警。

## 6. API 设计

所有接口沿用当前项目：认证中间件、`ApiResponse { success, data, error, meta }`、
OpenAPI、统一错误映射。路径中的 `chain_id` 必须为正整数。读取接口允许 read-only
principal；同步控制接口仅 admin。

### 6.1 MVP 接口

#### A. 区块详情

```http
GET /api/chains/{chain_id}/blocks/{block_ref}
```

`block_ref` 支持十进制高度、`0x` 高度和 64 位 block hash。第一版可只实现高度和 hash，
但必须在 OpenAPI 中明确未实现的形式。按 `(chain_id, block_number)` 或 hash 精确查 canonical
行；不存在返回 404，不调用 live RPC 临时补数据。

返回 `Block` 的全部已采集字段和 `transaction_count`。不嵌套完整交易数组，避免单个区块
返回体失控。

#### B. 区块交易列表

```http
GET /api/chains/{chain_id}/blocks/{block_ref}/transactions?limit=100&cursor=...
```

按 `transaction_index ASC` 稳定排序，cursor 为不透明 base64url token，包含 block number
和 index。`limit` 默认 100、最大 1000；cursor 失效或参数不匹配返回 400。响应 `meta`
至少包含 `limit`, `has_more`, `next_cursor`, `block_number`。

#### C. 交易详情

```http
GET /api/chains/{chain_id}/transactions/{tx_hash}
```

仅支持规范化 32-byte hash。交易未被采集或属于非 canonical fork 返回 404；不返回未采集
的 receipt/status/logs。响应包含 `block_number` 和 `transaction_index`，方便客户端定位。

#### D. 地址交易列表

```http
GET /api/chains/{chain_id}/addresses/{address}/transactions
  ?direction=any&from_block=&to_block=&limit=100&cursor=
```

`direction` 为 `from`、`to` 或 `any`，默认 `any`；创建合约交易因 `to_address=null` 只会
在 `from` / `any` 出现。按 `(block_number DESC, transaction_index DESC, tx_hash DESC)`
稳定排序，使用 keyset cursor，不允许 offset 深分页。为防止无界扫描，若存储未提供
address projection/index，必须要求 `from_block` 和 `to_block`；实现了 address index 后可
放宽该限制，但仍限制最大跨度和页大小。

### 6.2 建议的 P1 接口

这些接口只组合已采集字段，不需要新增 RPC 数据：

| 接口 | 用途 | 约束 |
| --- | --- | --- |
| `GET /api/chains/{chain_id}/blocks` | 区块范围列表 | `from_block`/`to_block` 必填，keyset 分页 |
| `GET /api/chains/{chain_id}/transactions` | 全链交易流 | block 范围必填，按 block/index 分页 |
| `GET /api/chains/{chain_id}/sync-status` | 同步进度 | 返回 checkpoint、safe head、lag、错误 |
| `GET /api/chains/{chain_id}/blocks/{block_ref}/summary` | 区块统计 | 只返回 tx count、gas/value 汇总；明确为数据集聚合 |

P1 不应在 MVP 失败时顺手实现；先保证四个 MVP 接口的数据正确性和查询计划。

### 6.3 失败与一致性语义

- ClickHouse 未启用：新数据 API 返回 503，并给出可操作错误；不回查 PostgreSQL。
- ClickHouse 查询失败：返回 502/503，不能返回空数组伪装成“没有数据”。
- `limit`、block 范围、地址/hash 格式、cursor 签名均在 handler 层校验。
- API 只读 canonical `FINAL` 数据；每个响应可在 `meta` 返回 `indexed_through_block`，
  让调用方知道数据新鲜度。
- JSON 大整数全部是字符串；不要因为某个字段当前值小而改成 number。

## 7. 管理与配置 API

collector 不应靠重启才能设定起始高度。建议提供 admin-only 接口：

```http
PUT /api/chains/{chain_id}/block-transaction-sync
{
  "start_block": 0,
  "realtime_enabled": true,
  "batch_size": 10,
  "max_concurrency": 2,
  "reorg_window": 32
}
GET /api/chains/{chain_id}/sync-status
POST /api/chains/{chain_id}/block-transaction-sync/pause
POST /api/chains/{chain_id}/block-transaction-sync/resume
```

如果实现者认为配置接口超出 MVP，至少要有 migration seed/环境变量初始化方式，并在
OpenAPI 和运维文档中写清楚。任何“默认从 0 全量同步”的隐式行为都不允许。

建议配置键：

```text
EVENTLAKE_BLOCK_TRANSACTION_ENABLED=false
EVENTLAKE_BLOCK_TRANSACTION_BATCH_SIZE=10
EVENTLAKE_BLOCK_TRANSACTION_MAX_CONCURRENCY=2
EVENTLAKE_BLOCK_TRANSACTION_REORG_WINDOW=32
EVENTLAKE_BLOCK_TRANSACTION_MAX_RESPONSE_BYTES=67108864
```

## 8. 迁移、测试与验收

### 8.1 迁移与启动

- 新增一份按当前日期命名的 PostgreSQL migration，创建 sync state，并补充约束和索引。
- `clickhouse/schema.sql` 以幂等 DDL 创建 `blocks`、`transactions` 及必要的 projection/index。
- startup 顺序保持：PostgreSQL migrations -> ClickHouse health check/schema -> worker。
- 旧 raw-log、decoded-event API 和表的行为不能回归。

### 8.2 最低测试矩阵

1. JSON-RPC block/transaction decoder：完整字段、缺失可选字段、空 input、uint256、非法
   hex、batch response id 错序和单项错误。
2. ClickHouse DDL、批量写入、重复写入、`FINAL` canonical replacement、reorg tombstone。
3. checkpoint：写入成功才推进；失败重试同一范围；部分 batch 不跳过高度。
4. API：四个 MVP 的正常、404、400、503、认证、分页 cursor、地址方向和大整数 JSON。
5. reorg：旧 fork 不可见，新 fork 可见，交易详情不会读到 stale row。
6. 回归：`cargo fmt --check`、默认 feature tests、`--features clickhouse` tests、真实
   ClickHouse integration test（环境可用时）。

### 8.3 验收标准

- 从指定 `start_block`（包含）开始，进程重启后无重复逻辑行、无跳块。
- 同一 batch 重试 3 次不会产生重复 API 结果。
- 人为替换最近区块 hash 后，旧 block/tx 在一次查询中消失，canonical fork 可查询。
- 交易 hash、地址、区块交易列表的排序和 cursor 在跨页时稳定。
- 任何 API 返回的字段都能在 `blocks` 或 `transactions` 中追溯，未采集字段明确为 null/不支持。
- ClickHouse 不可用时不会推进 checkpoint，也不会静默读 PostgreSQL。
- OpenAPI 和 `docs/USAGE.md` 能让操作者完成启用、初始化、暂停、检查和故障恢复。

## 9. 分阶段交付

1. **M0 基础模型与 RPC decoder**：类型、hex/uint 校验、单块和 batch 请求。
2. **M1 ClickHouse schema 与写入**：表、版本列、canonical 查询、集成测试。
3. **M2 checkpoint 与 collector**：safe head、并发、重试、reorg、恢复。
4. **M3 MVP read APIs**：四个查询接口、分页、OpenAPI、认证和错误语义。
5. **M4 运维面与回归**：sync status、pause/resume、指标、文档和完整测试。
6. **P1（可选）**：范围列表、全链交易流、address projection/index、聚合 summary。

只有 M0-M3 全部通过后才允许开始 P1；不能为了快速展示 API 而省略 reorg 和 checkpoint。
