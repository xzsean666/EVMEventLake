# ClickHouse Upgrade Design

Version: 1.0
Status: Completed
Date: 2026-08-04

## 1. 目标

在现有 PostgreSQL 架构基础上引入 ClickHouse，用于大规模事件数据的分析查询。

**两者分工：**

| 职责 | PostgreSQL | ClickHouse |
|---|---|---|
| 订阅、任务、检查点 | 是 | 否 |
| 用户、API Key、Auth | 是 | 否 |
| Chain、RPC 元数据 | 是 | 否 |
| ABI、合约注册 | 是 | 否 |
| 工作队列（decode、repair） | 是 | 否 |
| 原始日志（raw_logs） | 是（来源真相） | 是（搜索副本） |
| 解码事件（decoded_events） | 是（当前） | 是（分析查询主路径） |
| 地址索引（address_index） | 是（当前） | 是（按地址大范围搜索） |
| 事件字段索引（event_field_index） | 是（当前） | 是（按字段大范围搜索） |

ClickHouse 只接收来自 `indexing` 模块的数据副本，不是来源真相。所有修复/reorg 仍以 PostgreSQL 为准，修复后重新同步到 ClickHouse。

## 2. Cargo Feature

```toml
[features]
default = []
clickhouse = ["dep:clickhouse", "dep:time"]

[dependencies]
clickhouse = { version = "=0.13.3", optional = true, features = ["time", "uuid"] }
time = { version = "0.3.44", optional = true }
```

编译：
```
cargo build --release --features clickhouse
```

不带 `--features clickhouse` 的构建行为与现在完全一致。

## 3. 新增环境变量

```env
EVENTLAKE_CLICKHOUSE_HOST=localhost
EVENTLAKE_CLICKHOUSE_PORT=8123
EVENTLAKE_CLICKHOUSE_USER=eventlake
EVENTLAKE_CLICKHOUSE_PASSWORD=eventlake
EVENTLAKE_CLICKHOUSE_DB=eventlake
# 是否启用 ClickHouse（运行时开关，仅在 clickhouse feature 编译时有效）
EVENTLAKE_CLICKHOUSE_ENABLED=true
```

## 4. ClickHouse 表设计

### 4.1 decoded_events

```sql
CREATE TABLE IF NOT EXISTS decoded_events (
    id              UUID,
    raw_log_id      UUID,
    subscription_id Nullable(UUID),
    chain_id        UInt64,
    block_number    UInt64,
    block_hash      String,
    transaction_hash String,
    log_index       UInt32,
    contract_address String,
    event_name      String,
    topic0          String,
    abi_id          Nullable(UUID),
    indexed_fields  String,  -- JSON
    non_indexed_fields String,  -- JSON
    decoded_fields  String,  -- JSON
    is_removed      Bool     DEFAULT false,
    decoded_at      DateTime64(3, 'UTC'),
    indexed_at      DateTime64(3, 'UTC'),
) ENGINE = ReplacingMergeTree(indexed_at)
PARTITION BY (chain_id, toYYYYMM(toDateTime(intDiv(block_number, 5) + 1438300000)))
ORDER BY (chain_id, block_number, log_index);
```

> 注：block_number 分区用 ReplacingMergeTree，reorg 后用新记录覆盖旧记录。

### 4.2 address_index

```sql
CREATE TABLE IF NOT EXISTS address_index (
    chain_id        UInt64,
    address         String,
    block_number    UInt64,
    transaction_hash String,
    log_index       UInt32,
    event_name      String,
    contract_address String,
    role            String,  -- 'emitter' | 'field'
    field_name      String,
    is_removed      Bool DEFAULT false,
    indexed_at      DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (chain_id, address, block_number, transaction_hash, log_index, field_name);
```

### 4.3 event_field_index

```sql
CREATE TABLE IF NOT EXISTS event_field_index (
    chain_id        UInt64,
    topic0          String,
    field_name      String,
    field_value     String,
    block_number    UInt64,
    transaction_hash String,
    log_index       UInt32,
    is_removed      Bool DEFAULT false,
    indexed_at      DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (chain_id, topic0, field_name, field_value, block_number, transaction_hash, log_index);
```

DDL 文件路径：`clickhouse/schema.sql`

## 5. 模块实施顺序

每个模块完成后写对应交接记录到 `docs/CLICKHOUSE_HANDOVER.md`。

| 序号 | 模块 | 文件 | 说明 |
|---|---|---|---|
| M0 | Docker 基础设施 | Dockerfile.clickhouse 等 6 个文件 | **已完成** |
| M1 | Cargo feature + 依赖 | `Cargo.toml` | 声明 feature，添加 clickhouse crate |
| M2 | configuration | `src/configuration/mod.rs` | 添加 `ClickHouseConfig` 结构体 |
| M3 | clickhouse 连接模块 | `src/clickhouse/mod.rs`（新建） | Client 封装、healthcheck、初始化 DDL |
| M4 | ApplicationState | `src/app/application_state.rs` | 添加 `Option<clickhouse::Client>` |
| M5 | startup | `src/app/startup.rs` | 连接初始化，feature gate |
| M6 | ClickHouse DDL | `clickhouse/schema.sql` | 建表语句，startup 时执行 |
| M7 | indexing 双写 | `src/indexing/mod.rs` | decoded_events / address_index / event_field_index 写入 ClickHouse |
| M8 | search 路由 | `src/search/mod.rs` | 有 ClickHouse 时走 ClickHouse，否则保持 PostgreSQL |
| M9 | 集成测试 | `tests/clickhouse_*.rs` | 端到端验证双写和查询路由 |

## 6. 数据一致性策略

- **写入顺序**：先提交 PostgreSQL，成功后写 ClickHouse。ClickHouse 写失败记录到日志，不回滚 PostgreSQL 事务。
- **Reorg 修复**：PostgreSQL 标记 removed 后，重新 trigger indexing 写新状态到 ClickHouse（ReplacingMergeTree 用 `indexed_at` 去重）。
- **查询路由**：`search` 模块编译期通过 `#[cfg(feature = "clickhouse")]` 决定查询路径，运行时通过 `AppState.clickhouse` 是否为 `Some` 再次判断。

## 7. 测试策略

见 `docs/CLICKHOUSE_HANDOVER.md` M9 节。总体要求：
- 每个模块 unit test 覆盖配置解析和结构体
- M7 indexing 双写需要集成测试（docker-compose.clickhouse.yml 启动环境）
- M8 search 路由需要 e2e 测试：写入 → 搜索 → 验证结果来自 ClickHouse
- 最终运行 `cargo test --features clickhouse` 全量通过

## 8. 不影响现有行为的约束

- 不加 `--features clickhouse` 时，编译和运行行为与 V1 完全相同
- PostgreSQL 模块不依赖 ClickHouse 模块
- ClickHouse 连接失败只打 warn 日志，不 panic，服务仍可启动（降级到 PostgreSQL 查询）
