# EVMEventLake 系统架构说明 (ARCHITECTURE)

本文档是 AI Agent 在本仓库进行工程开发时的**系统架构权威事实来源**。

---

## 1. 系统概述与核心定位

**EVMEventLake** 是一个基于 Rust 构建的高性能、高可靠 EVM 原始事件日志（Raw Event Logs）及链上数据采集、索引与检索系统。

- **核心定位**：专注于 Raw Event Lake。核心引擎保证 EVM 日志与区块交易数据的原始、有序、完整落地与可检索性，不强行耦合易变的应用层 ABI 解码（解码工作交由下游消费方自由演进）。
- **架构形态**：Rust 单体优先（Monolith First），部署轻量、可测性高、生命周期集中管理。
- **存储方案**：双存储模式。
  - **PostgreSQL**：全局运维状态的唯一事实源（链、RPC 节点池、订阅定义、Checkpoints、Reorg 观测点、用户与鉴权）。
  - **ClickHouse（可选）**：海量原始日志与区块交易数据的分析型主存储，支持全链无地址过滤（`all_events`）的高通量写入与高效检索。在未启用 ClickHouse 时，原始日志存储在 PostgreSQL 分区表中。

---

## 2. 系统整体架构拓扑

```text
               +---------------------------------------------+
               |        Client / API Consumer / Downstream    |
               +---------------------------------------------+
                                      |
                                      v
+-------------------------------------------------------------------------------+
|                               Rust Monolith Core                              |
|                                                                               |
|  +-------------------------------------------------------------------------+  |
|  |                           API & Auth Layer                              |  |
|  |  - REST Endpoints (Subscriptions, Raw-Logs Search, Chains, RPC, Blocks) |  |
|  |  - Unified Search DSL Compiler (Strict Whitelist-driven)                |  |
|  |  - JWT & API Key Authentication                                         |  |
|  +-------------------------------------------------------------------------+  |
|                                      |                                        |
|  +-------------------------------------------------------------------------+  |
|  |                       Background Runtime & Workers                      |  |
|  |  - RPC Pool Manager: Endpoint Health Checks & Circuit Breaker           |  |
|  |  - Collector Worker: Dynamic Multi-Address Carpool & All-Events Polling |  |
|  |  - Reorg Worker: Block Hash Verification & Atomic Rewind                |  |
|  |  - Block & Transaction Collector (Extended Pipeline)                    |  |
|  +-------------------------------------------------------------------------+  |
+-------------------------------------------------------------------------------+
                 |                                             |
                 v                                             v
+---------------------------------+           +---------------------------------+
|      PostgreSQL (Operational)   |           |    ClickHouse (Raw Lake / Opt)  |
| - Chains & RPC Pool metadata    |           | - `raw_logs` (ReplacingMergeTree|
| - Subscriptions & Checkpoints   |           |   ordered by chain,block,tx,idx)|
| - Reorg observations            |           | - `blocks` & `transactions`     |
| - Raw logs (when CH disabled)   |           | - Bloom filter skipping indexes |
| - Users & API Keys              |           | - Tombstone-based reorg repair  |
+---------------------------------+           +---------------------------------+
```

---

## 3. 存储模式规范 (Storage Modes)

系统通过编译 Feature 和运行时环境变量严格控制存储模式：

| 数据职责 | PostgreSQL 基础模式 | ClickHouse 高通量模式 |
| :--- | :--- | :--- |
| **构建特征与开关** | 默认 `cargo build` | `--features clickhouse` 且 `EVENTLAKE_CLICKHOUSE_ENABLED=true` |
| **Chains / RPC / Subscriptions / Checkpoints** | PostgreSQL | PostgreSQL (唯一事实源) |
| **Reorg 状态跟踪** | PostgreSQL | PostgreSQL 协调 + ClickHouse 写入 Tombstone |
| **原始日志存储 (`raw_logs`)** | PostgreSQL 分区表 | ClickHouse `raw_logs` 表 |
| **全链无过滤采集 (`all_events`)** | 不支持 (防单库打爆) | 完整支持 |
| **查询机制** | PostgreSQL SQL 查询 | ClickHouse `FINAL` 引擎查询，自动排除 Tombstones |

> **关键约束**：未开启 ClickHouse feature 时若将 `EVENTLAKE_CLICKHOUSE_ENABLED=true` 会触发启动硬失败，防止误将海量全链流量倾泻到 PostgreSQL。

---

## 4. 核心模块职责与边界

所有代码必须严格遵循模块边界，严禁跨层随意暴露内部实现：

1. **`app` / `configuration`**
   - 负责进程引导、统一类型化配置加载（前缀 `EVENTLAKE_`）、依赖注入与优雅停机。
2. **`database`**
   - 封装 PostgreSQL 连接池、事务管理与 SQLx 迁移（`migrations/`）。
3. **`clickhouse`**
   - 封装 ClickHouse 客户端、DDL 幂等初始化、批量写入缓冲、Tombstone 同步与专用查询。
4. **`chains` & `rpc_pool`**
   - 维护区块链元数据；实现独立 RPC 节点健康检查、自适应权重、限流与故障熔断。
5. **`subscriptions`**
   - 负责订阅意图管理与 Checkpoint 推进；支持按合约地址订阅（`contract`）及全链订阅（`all_events`）。
6. **`collector`**
   - 负责高吞吐 `eth_getLogs` 抓取；支持多订阅动态拼车（Carpooling，同一高度合乘单次 RPC 请求）与结果分流（Demux）。
   - **核心一致性原则**：必须在原始存储（PG 或 CH）批量持久化成功后，才允许提交推进 PostgreSQL Checkpoint。
7. **`reorg`**
   - 维护区块哈希检查点，检测分叉事件；触发时原子回退 Checkpoint，并在存储层标记 Tombstone（`is_removed = true`）。
8. **`search`**
   - 统一 Search DSL 编译器。采用严格字段白名单验证（`chain_id` 必须提供，支持 `topic0`~`topic3` 32 字节标准化精确匹配），防范 SQL 注入与全表扫描。
9. **`block_transaction`**
   - 全链规范区块与交易数据的独立抓取与查询模块。

---

## 5. 核心工作流与一致性保障

### 5.1 采集与存储一致性
```text
Checkpoint 读取 -> RPC eth_getLogs -> 校验区块哈希
                          |
             [写入选定存储: PG 或 ClickHouse]
                          |
             写入成功 ? -> 是: 原子推进 PostgreSQL Checkpoint
                        -> 否: 丢弃当前客户端/连接，保留原 Checkpoint，下个 Tick 重试
```

### 5.2 Reorg 修复工作流
- 发现当前高度区块 Hash 与链上 RPC 返回不一致。
- 确定分叉点高度 `reorg_block`。
- 回退 PostgreSQL Checkpoint 至 `reorg_block - 1`。
- PostgreSQL 模式下更新行标记 `removed = true`；ClickHouse 模式下批量写入高版本 Tombstone（`is_removed = true`）。
- 查询层统一附加 `FINAL` 与 `is_removed = false`，保证瞬时一致性。

---

## 6. 构建、测试与运行命令

- **代码格式化**：`cargo fmt --check`
- **静态分析与类型检查**：`cargo check --all-targets`
- **ClickHouse Feature 编译检查**：`cargo check --locked --features clickhouse`
- **单元测试与集成测试**：`cargo test --locked`
