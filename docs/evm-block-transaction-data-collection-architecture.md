
# EVM Block & Transaction 数据采集架构

## 1. 目标

通过 EVM JSON-RPC 全量/实时采集：

* Block
* Transaction

存储到 ClickHouse，为后续 `txlist`、地址交易查询、Logs、Token Transfer 等功能提供基础数据。

---

## 2. RPC

使用：

```text
eth_getBlockByNumber(blockNumber, true)
```

一次获取：

```text
Block + Block 内全部 Transactions
```

通过 JSON-RPC Batch 一次请求多个 Block。

支持：

* RPC Pool
* Batch
* 并发
* 重试
* 断点续传
* Reorg 检测

---

## 3. Block 数据

ClickHouse：

```text
blocks

chain_id
block_number
block_hash
parent_hash
timestamp

gas_limit
gas_used
base_fee_per_gas

beneficiary

transactions_root
receipts_root
state_root

size

withdrawals_root
blob_gas_used
excess_blob_gas
parent_beacon_block_root
```

一 Block 一行。

### 硬盘

Ethereum 全历史：

**约 5～20 GB（ClickHouse 压缩后）**

Block 数据占用很小，不需要为了节省空间过度删字段。

---

## 4. Transaction 数据

ClickHouse：

```text
transactions

chain_id

tx_hash

block_number
transaction_index

from_address
to_address

value

nonce
gas
gas_price

max_fee_per_gas
max_priority_fee_per_gas

tx_type

method_id
```

一 Transaction 一行。

暂时不保存完整 `input/calldata`。

Hash 和 Address 使用 Binary / FixedString 存储。

### 硬盘

Ethereum 全历史约 30 亿 Transaction：

**约 100～300 GB（ClickHouse 压缩后）**

实际占用取决于 ClickHouse 排序、Codec、字段以及数据版本。

---

## 5. 总硬盘预算

只保存：

```text
blocks
transactions
```

预计：

```text
Block          ~5–20 GB
Transactions   ~100–300 GB
────────────────────────
合计           ~105–320 GB
```

建议使用：

**至少 1TB SSD**

如果考虑后续增加：

```text
logs
receipts
token_transfers
traces
address index
```

建议直接：

**2TB SSD 或更大。**

---

## 6. ClickHouse

```text
blocks
    ORDER BY (chain_id, block_number)

transactions
    ORDER BY (chain_id, block_number, transaction_index)
```

数据采用结构化字段存储，不保存完整 RPC JSON。

---

## 7. 同步流程

```text
RPC
 ↓
Batch eth_getBlockByNumber
 ↓
Decode
 ├── Block → blocks
 └── Transactions → transactions
 ↓
ClickHouse Bulk Insert
```

记录：

```text
last_indexed_block
```

支持中断后继续同步。

---

## 8. 后续扩展

第一阶段：

```text
blocks
transactions
```

后续：

```text
logs
receipts
token_transfers
NFT
traces
address index
historical balance
```

Logs 使用：

```text
eth_getLogs
```

单独采集。

---

## 9. 第一阶段目标

实现一个高性能 Rust Collector：

```text
EVM RPC
  ↓
Batch + 并发
  ↓
Block / Transaction Decoder
  ↓
ClickHouse
```

重点：

**高吞吐、低硬盘占用、可断点恢复、可扩展。**
