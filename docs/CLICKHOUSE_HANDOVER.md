# ClickHouse 升级交接记录

完成日期：2026-08-04

## M10 - Raw event lake upgrade [done]

完成日期：2026-08-05

- ClickHouse mode 的 raw log 已改为直接写入 `raw_logs`，不再写入 PostgreSQL
  `eventlake_raw_logs` 或 `eventlake_decode_queue`。PostgreSQL 继续保存订阅、checkpoint、
  reorg block hash、RPC 和认证等运行状态。
- background runtime 不再启动 decoder；ABI 解码由下游项目完成。旧的 decoded-event 表和
  API 仅保留为升级前历史数据的兼容读取面，不会收到新采集数据。
- `raw_logs` 使用 `ReplacingMergeTree(stored_at)`，主键为
  `(chain_id, block_number, transaction_hash, log_index)`，保存完整 topics/data，并增加
  `topic0` 到 `topic3` 列和 bloom skipping indexes。
- ClickHouse raw 写入成功前不会推进 PostgreSQL checkpoint。写入失败清除 client，保留
  checkpoint 并在下次 tick 重拉同一段区块。reorg 使用 `is_removed=true` tombstone，查询
  使用 `FINAL`。
- subscription 新增 `collection_scope`：`contract` 保持现有按地址采集；`all_events`
  不带 `eth_getLogs.address`，仅允许 ClickHouse 模式。迁移文件为
  `202608050001_raw_event_lake.sql`。
- 两种 scope 都使用订阅请求中的 `start_block` 初始化 checkpoint，并从该区块（包含）
  开始采集；`EVENTLAKE_CLICKHOUSE_ENABLED` 只选择 raw storage，不会自动创建
  `all_events` 订阅。
- 新增订阅 batch endpoint，可只传 `contract_addresses` 和 `start_block` 批量创建合约
  raw 订阅，不需要 ABI；输入地址和已有 active subscription 都会去重。
- 新增 `POST /api/raw-logs/search`，支持 `chain_id`（必须 `eq`）、`block_number`、
  `contract_address`、`transaction_hash`、`topic0` 到 `topic3`。返回原始 topics 和 data。
- `tests/search_dsl_tests.rs` 增加 raw 查询 DSL 校验；ClickHouse 集成测试增加 raw 写入与
  `topic0` 查询覆盖。

### 当前验证限制

本工作环境没有 `cargo`/Rust 工具链，因此尚未在此环境执行 `cargo fmt`、`cargo check` 或
ClickHouse integration test。提交或部署前应运行 `docs/CLICKHOUSE_UPGRADE.md` 第 9 节命令。

已使用本地 `clickhouse/clickhouse-server:24.8` 执行 `clickhouse/schema.sql`，并验证
`raw_logs` insert、`FINAL` 查询及 reorg tombstone insert 语句。

## M0 - Docker 基础设施 [done]

新增 ClickHouse 源码构建、预编译和国内镜像三套 Dockerfile/Compose 变体。ClickHouse 使用 `24.8`，HTTP 端口为 `8123`，healthcheck 为 `/ping`，数据和日志分别挂载到 `data/clickhouse` 与 `logs/clickhouse`。

## M1 - Cargo feature + 依赖 [done]

- 新增空默认 feature 与可选 `clickhouse` feature。
- 固定 `clickhouse = 0.13.3`，启用 `time` / `uuid` 映射；项目公共时间类型仍使用 `chrono`。
- `cargo check --locked --features clickhouse` 已通过。

## M2 - configuration [done]

- 新增 `ClickHouseConfig { host, port, user, password, database, enabled }`。
- 使用 `EVENTLAKE_CLICKHOUSE_` 环境变量前缀，HTTP 端口默认 `8123`，运行时默认关闭。
- `ClickHouseConfig::url` 单元测试已覆盖 endpoint 拼接。

## M3 - clickhouse 连接模块 [done]

- 新建 `src/clickhouse/mod.rs`，提供连接、DDL 初始化、事件写入、reorg 同步和搜索 API。
- `connect` 先执行 `SELECT 1` healthcheck，再执行 `clickhouse/schema.sql`。
- 搜索使用 `FINAL`，使 tombstone 在后台 merge 前生效。

## M4 - ApplicationState [done]

- `ApplicationState` 在 feature gate 下新增 `Option<clickhouse::Client>` 与 `with_clickhouse` builder。
- 不启用 feature 时结构体和构建路径不变。

## M5 - startup [done]

- PostgreSQL migration 完成后初始化 ClickHouse。
- 连接失败只记录 `warn`，不阻止服务启动；raw-log 写入会保留 checkpoint 并在 ClickHouse
  恢复后重试，不会回退到 PostgreSQL。

## M6 - ClickHouse DDL [done]

- `clickhouse/schema.sql` 创建 `decoded_events`、`address_index`、`event_field_index`。
- 三张表使用 `ReplacingMergeTree(indexed_at)`，并包含 `is_removed` tombstone；主表额外保留 `raw_log_id`、JSON 字段和 `decoded_at` 以保持现有搜索返回结构。
- startup 自动执行 `CREATE TABLE IF NOT EXISTS`。

## M7 - ClickHouse-only derived writes [retired]

这部分是升级前的 ABI 解码投影流程，现仅保留旧表和旧 API 供历史数据读取。当前版本不再
写入 decode queue、decoded events、address index 或 event-field index；reorg 只对
ClickHouse `raw_logs` 写入 `is_removed = true` tombstone，canonical fork 重新采集后写回
`false` 版本。

## M8 - search 路由 [done]

- 运行时模式决定查询存储：PostgreSQL-only 查询 PostgreSQL，ClickHouse 模式查询 ClickHouse。
- ClickHouse 不可用或查询失败时不回退，以避免不完整结果和双份派生数据。
- 已保留原有 filter/sort 语义，并使用地址/字段索引子查询。

## M9 - 集成测试 [done]

- 测试文件：`tests/clickhouse_integration_tests.rs`。
- 覆盖配置 endpoint、healthcheck/schema 初始化路径、事件/地址/字段写入、ClickHouse 搜索路由和 tombstone 隐藏。
- 运行命令：`cargo test --locked --features clickhouse`；集成测试默认跳过，设置 `EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true` 后连接真实服务。
- 本次已使用 `clickhouse/clickhouse-server:24.8` 实际执行连接、DDL、写入、搜索和 tombstone 测试，结果通过。

## 验证记录

- `cargo check --locked --features clickhouse`：通过。
- `cargo test --locked --features clickhouse --test clickhouse_integration_tests`：通过，`1 passed`。
- `cargo test --locked --features clickhouse --lib`：通过，`5 passed`。
- `cargo test --locked --features clickhouse`：通过，所有测试 green。
- `git diff --check`：通过。
