# EVMEventLake 系统架构说明 (ARCHITECTURE)

本文档是 AI Agent 在本仓库进行工程开发时的**系统架构权威事实来源**。

---

## 1. 系统概述与核心定位

**EVMEventLake** 是一个基于 Rust 构建的高性能、高可靠 EVM 原始事件日志（Raw Event Logs）及链上数据采集、索引与检索系统。

- **核心定位**：专注于 Raw Event Lake。核心引擎保证 EVM 日志与区块交易数据的原始、有序、完整落地与可检索性，不强行耦合易变的应用层 ABI 解码（解码工作交由下游消费方自由演进）。
- **架构形态**：Rust 单体优先（Monolith First），部署极简、零外部 RDBMS 依赖、生命周期集中管理。
- **存储方案**：纯净分层单一架构。
  - **SQLite**：以单文件嵌入式运行在 Rust 单体进程内，作为运维配置、RPC 节点池、订阅、Checkpoints 与鉴权的轻量事务事实源，彻底摆脱外部 PostgreSQL 容器依赖。
  - **ClickHouse**：系统唯一且强依赖的分析型原始数据湖引擎，负责承载全链无过滤（`all_events`）海量原始日志与区块交易数据的高通量写入与高效 DSL 检索。
  - **统一 S3 增量备份与恢复**：构建 SQLite 在线零停机快照 + ClickHouse 分片（Parts）增量上传的统一云端/本地备份工具链。

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
|     Embedded SQLite (Control)   |           |    ClickHouse (Raw Event Lake)  |
| - Chains & RPC Pool metadata    |           | - `raw_logs` (ReplacingMergeTree|
| - Subscriptions & Checkpoints   |           |   ordered by chain,block,tx,idx)|
| - Reorg observations            |           | - `blocks` & `transactions`     |
| - Users & API Keys              |           | - Bloom filter skipping indexes |
| - WAL mode / VACUUM INTO atomic |           | - Native BACKUP / RESTORE S3    |
+---------------------------------+           +---------------------------------+
```

---

## 3. 存储职责划分 (Storage Responsibilities)

| 数据职责 | 存储引擎 | 物理模型与机制 |
| :--- | :--- | :--- |
| **Chains / RPC / Subscriptions / Checkpoints** | SQLite | 嵌入式单文件数据库（WAL 模式），单体进程内毫秒级事务。 |
| **Reorg 状态跟踪** | SQLite + ClickHouse | SQLite 记录分叉高度并原子回退 Checkpoint；ClickHouse 批量写入高版本 Tombstone（`is_removed = true`）。 |
| **原始日志存储 (`raw_logs`)** | ClickHouse | ReplacingMergeTree 引擎，以 `(chain_id, block_number, transaction_index, log_index)` 为排序主键。 |
| **区块与交易 (`blocks`, `transactions`)** | ClickHouse | ReplacingMergeTree 引擎，支持哈希点查、高度范围检索与地址过滤。 |
| **全链无过滤采集 (`all_events`)** | ClickHouse | 核心支持全量采集，批量写入 ClickHouse 数据湖。 |
| **查询机制** | ClickHouse | `FINAL` 引擎查询，自动过滤 `is_removed = true` 的分叉数据。 |
| **备份与恢复** | SQLite + ClickHouse | SQLite `VACUUM INTO` + ClickHouse 原生分片增量快照，统一上传 S3 或本地归档。 |

---

## 4. 核心模块职责与边界

所有代码必须严格遵循模块边界，严禁跨层随意暴露内部实现：

1. **`app` / `configuration`**
   - 负责进程引导、统一类型化配置加载（前缀 `EVENTLAKE_`）、依赖注入与优雅停机。
2. **`database`**
   - 封装 SQLite 连接池（`SqlitePool`）、WAL 模式配置与 SQLx 迁移（`migrations/`）。
3. **`clickhouse`**
   - 封装 ClickHouse 客户端、DDL 幂等初始化、批量写入缓冲、Tombstone 同步与专用查询。
4. **`chains` & `rpc_pool`**
   - 维护区块链元数据；实现独立 RPC 节点健康检查、自适应权重、阶梯退避与故障熔断。
5. **`subscriptions`**
   - 负责订阅意图管理与 Checkpoint 推进；支持按合约地址订阅（`contract`）及全链订阅（`all_events`）。
6. **`collector`**
   - 负责高吞吐 `eth_getLogs` 抓取；支持多订阅动态拼车（Carpooling，同一高度合乘单次 RPC 请求）与结果分流（Demux）。
   - **核心一致性原则**：必须在 ClickHouse 批量持久化成功后，才允许提交推进 SQLite Checkpoint。
7. **`reorg`**
   - 维护区块哈希检查点，检测分叉事件；触发时原子回退 SQLite Checkpoint，并在 ClickHouse 标记 Tombstone（`is_removed = true`）。
8. **`search`**
   - 统一 Search DSL 编译器。采用严格字段白名单验证（`chain_id` 必须提供，支持 `topic0`~`topic3` 32 字节标准化精确匹配），防范全表扫描。
9. **`block_transaction`**
   - 全链规范区块与交易数据的独立抓取与查询模块。

---

## 5. 核心工作流与一致性保障

### 5.1 采集与存储一致性
```text
Checkpoint 读取 (SQLite) -> RPC eth_getLogs -> 校验区块哈希
                                    |
                       [批量写入 ClickHouse raw_logs]
                                    |
                       写入成功 ? -> 是: 原子推进 SQLite Checkpoint
                                  -> 否: 保留原 Checkpoint，下个 Tick 重试
```

### 5.2 Reorg 修复工作流
- 发现当前高度区块 Hash 与链上 RPC 返回不一致。
- 确定分叉点高度 `reorg_block`。
- 回退 SQLite Checkpoint 至 `reorg_block - 1`。
- ClickHouse 批量写入高版本 Tombstone（`is_removed = true`）。
- 查询层统一附加 `FINAL` 与 `is_removed = false`，保证瞬时一致性。

---

## 6. 构建、测试与运行命令

- **代码格式化**：`cargo fmt --check`
- **静态分析与类型检查**：`cargo check --ignore-rust-version --all-targets`
- **单元测试与集成测试**：`cargo test --ignore-rust-version`
- **一键备份恢复端到端测试**：`./tests/test_backup_restore_e2e.sh`
