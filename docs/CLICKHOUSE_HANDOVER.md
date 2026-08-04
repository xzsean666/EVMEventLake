# ClickHouse 升级交接记录

完成日期：2026-08-04

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
- 连接失败只记录 `warn`，保留 PostgreSQL fallback，不阻止服务启动。

## M6 - ClickHouse DDL [done]

- `clickhouse/schema.sql` 创建 `decoded_events`、`address_index`、`event_field_index`。
- 三张表使用 `ReplacingMergeTree(indexed_at)`，并包含 `is_removed` tombstone；主表额外保留 `raw_log_id`、JSON 字段和 `decoded_at` 以保持现有搜索返回结构。
- startup 自动执行 `CREATE TABLE IF NOT EXISTS`。

## M7 - indexing 双写 [done]

- decoder 先提交 PostgreSQL decoded event、派生索引和 queue 状态，再写 ClickHouse 副本。
- ClickHouse 写失败只记录 `warn`，不回滚 PostgreSQL。
- reorg 后从 PostgreSQL 读取 `decode_status = 'reorged'` 事件，写入相同排序键的 `is_removed = true` 版本；canonical fork 重解码后写回 `false` 版本。

## M8 - search 路由 [done]

- feature 构建且运行时存在 client 时优先查询 ClickHouse。
- ClickHouse 不可用、查询失败或运行时关闭时回退既有 PostgreSQL executor。
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
