# Block / Transaction 升级任务拆分

## 1. 使用方式

本文件把升级拆成 Gemini 3.7 Flash 可以逐项完成、逐项验证的任务。每个任务都限定了
输入、文件边界、实现目标、验收命令和交接产物。任务必须按依赖顺序执行；一个任务未
通过验收时，不要跳到后续任务或同时做大范围重构。

全局约束：

- 只实现 `docs/block-transaction-upgrade/01-upgrade-design.md` 约定的 block/transaction 范围。
- 当前 `src/collector` 已经是可用的 log 收集模块；本任务不新增或重写 log collector，
  不改变 `eth_getLogs`、raw-log checkpoint、raw-log reorg 和 raw-log API。
- 复用现有 Axum、SQLx、reqwest、tracing、ApplicationError、ApplicationState、RPC
  endpoint 选择器和 ClickHouse client 风格。
- 不改 raw-log 的语义，不恢复 ABI decoder，不新增 receipts/logs/traces/input 全文采集。
- 所有迁移和 DDL 必须幂等；所有写入必须可重试。
- 每个任务结束都运行它自己的最小验证，并在交接记录中报告实际结果。

## 2. 依赖图

```text
T0 现状盘点与接口冻结
 ├── T1 数据类型与 RPC batch
 └── T2 PostgreSQL sync migration
       └── T3 ClickHouse schema/writer
             └── T4 collector/checkpoint/reorg
                   └── T5 read APIs
                         └── T6 admin/status/observability
                               └── T7 integration/regression/release docs
```

T1 和 T2 可以在 T0 之后分别进行，但提交前必须合并验证。T3 依赖 T1 的字段模型；T4
依赖 T2、T3；T5 依赖 T3 的查询函数；T6/T7 依赖前面所有任务。

## 3. 任务清单

### T0：现状盘点与接口冻结

**目标**：让实现者先理解仓库，而不是直接创建平行架构。

**阅读范围**：

- `docs/ARCHITECTURE.md`
- `docs/CLICKHOUSE_UPGRADE.md`
- `docs/evm-block-transaction-data-collection-architecture.md`
- `src/app/application_state.rs`
- `src/app/startup.rs`
- `src/rpc_pool/evm_rpc_client.rs`
- `src/collector/worker.rs`（只确认现有 log worker 的生命周期和不可回归边界）
- `src/search/mod.rs`
- `clickhouse/schema.sql`
- 所有 migrations 的最新状态

**产物**：在 `docs/block-transaction-upgrade/` 之外的实现前，先输出一份不超过 2 页的
`T0-notes.md`（可放在临时分支，不必提交），列出拟改文件、冲突点、当前 ClickHouse
模式、当前 API envelope 和测试命令。

**验收**：没有代码修改；能指出 subscription checkpoint 不能复用的原因；能说明
ClickHouse 不可用时的行为。

### T1：数据类型与 JSON-RPC block batch client

**目标**：建立严格、可测试的 RPC 层，不写数据库。

**建议文件**：

- 修改 `src/rpc_pool/evm_rpc_client.rs` 或新增同目录模块。
- 在 `src/shared/hex.rs` 增加可复用但不破坏现有行为的校验函数。
- 新增针对 decoder/batch 的单元测试。

**实现要求**：

- 定义 Block、Transaction、可选字段和 JSON-RPC batch response 类型；serde 使用
  `camelCase` 反序列化。
- 支持 `eth_getBlockByNumber(block, true)` 单块和 batch；用 request id 对应响应。
- 解析 hex quantity、hash、address、method id；拒绝负数、错误长度、溢出和错误 RPC response。
- 保留原始十进制字符串，不能把 `uint256` 解析成 Rust `u64`。
- 处理 `miner`/`author`、缺失 `baseFeePerGas`、`withdrawalsRoot`、blob 字段和 `to=null`。
- 将 HTTP/RPC 错误转换成现有 `ApplicationError`，保留可诊断信息但不泄露密钥。

**不做**：ClickHouse、checkpoint、receipt、input 全文。

**验收命令**：

```bash
cargo fmt --check
cargo test --locked --lib rpc_pool
```

**交接**：列出新增类型、batch 部分失败的处理约定、未覆盖的 provider 特例。

### T2：PostgreSQL sync state migration

**目标**：为每条链提供独立的 block/transaction checkpoint 和配置状态。

**建议文件**：

- 新增 `migrations/YYYYMMDD0001_block_transaction_sync.sql`。
- 新增 `src/block_transaction/mod.rs` 或 `src/block_transaction/state.rs` 的查询模型。

**实现要求**：

- sync state 使用 `chain_id` 主键，包含 `next_block`、`safe_head`、`latest_seen_block`、
  status、last_error、last_success_at、updated_at。
- 对高度、window、concurrency、reorg window 加非负/范围约束。
- 迁移可重复执行，不能修改既有 raw-log 表的语义。
- 初始化方式必须明确：admin 配置接口或显式 migration/环境变量；禁止隐式从 0 开始。
- 状态更新函数要能在一个数据库事务中表达“写入成功后推进”。

**验收命令**：

```bash
cargo test --locked --lib
```

有 PostgreSQL 环境时额外执行 migration smoke test。交接中给出 SQL 表结构和状态转移图。

### T3：ClickHouse blocks/transactions schema 与 writer

**目标**：实现数据面和幂等写入，不启动 worker。

**建议文件**：

- 修改 `clickhouse/schema.sql`。
- 修改或新增 `src/clickhouse/block_transaction.rs`。
- 在 `src/clickhouse/mod.rs` 暴露初始化、批量写入、详情和列表查询所需的最小函数。

**实现要求**：

- `blocks` 和 `transactions` 使用 `ReplacingMergeTree(stored_at)`，有明确 partition、
  ORDER BY、canonical/version 字段。
- 可选字段使用 Nullable；API 需要的 uint256 不能丢精度。
- 为 block hash、tx hash、from/to address 建 skipping index 或等价访问路径；区块交易
  列表必须命中主排序键。
- 批量写入使用显式列名；空 batch 不执行无意义 INSERT；重试不会产生重复逻辑结果。
- 所有 canonical 查询使用 `FINAL` 或经过证明等价的 latest-version 逻辑。
- 在真实 ClickHouse 可用时新增集成测试：DDL、insert、重复 insert、reorg replacement、
  count 和精确查询。

**验收命令**：

```bash
cargo fmt --check
cargo check --locked --features clickhouse
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true \
  cargo test --locked --features clickhouse --test clickhouse_integration_tests -- --nocapture
```

**交接**：记录表 schema、ReplacingMergeTree identity、地址查询是否需要 projection/index
以及实测查询计划。

### T4：collector、checkpoint、safe head 和 reorg

**目标**：接入后台 worker，完成连续采集和故障恢复。

**建议文件**：

- 新增 `src/block_transaction/worker.rs`、`collector.rs` 或等价模块。
- 修改 `src/background/mod.rs`、`src/app/startup.rs`、`src/app/application_state.rs`、
  `src/configuration/mod.rs`。
- 必要时在 `src/reorg` 增加有测试保护的通用 block hash 协调函数，但不要改变 raw-log
  合约；不要把新逻辑塞入 `src/collector/worker.rs`。

**实现要求**：

- 只采集到 `safe_head`；caught up 后继续 realtime tick。
- batch/并发/response byte 上限配置化；provider 错误要拆小并退避。
- 只有 ClickHouse 完整写入成功后才推进 PostgreSQL `next_block`。
- batch 部分成功不能跳过缺失高度；重试使用相同范围。
- 校验请求高度、返回高度、parent_hash 连续性和 chain id。
- reorg 时写 canonical replacement/tombstone，暂停到写入成功；API 不能看到 stale fork。
- 复用 `select_rpc_endpoint` 和 endpoint failure 状态；日志包含 chain/range/attempt。
- 默认开关关闭，不影响既有 worker 和默认 PostgreSQL-only 部署。

**验收**：

- 单元测试覆盖 safe head、window shrink、部分 batch、checkpoint failure。
- 使用 mock RPC + ClickHouse 测试重启、重复 tick、reorg；同时确认现有 log worker 的
  测试和行为没有变化。
- 运行默认 feature 和 clickhouse feature 的现有测试。

**交接**：提供状态转移图、重试退避规则、手工暂停/恢复方式。

### T5：MVP read APIs

**目标**：只暴露已采集字段，接口稳定且可分页。

**建议文件**：

- 新增 `src/block_transaction/api.rs` 或 `src/block_transaction/mod.rs` 路由。
- 修改 `src/api/routes.rs`，合并 routes/openapi。
- 复用 `src/api/response.rs`、`src/auth`、`src/shared/pagination.rs`、validation。

**必须实现**：

1. `GET /api/chains/{chain_id}/blocks/{block_ref}`
2. `GET /api/chains/{chain_id}/blocks/{block_ref}/transactions`
3. `GET /api/chains/{chain_id}/transactions/{tx_hash}`
4. `GET /api/chains/{chain_id}/addresses/{address}/transactions`

**实现要求**：

- OpenAPI schema、参数限制、400/404/503、认证角色全部明确。
- block transaction 按 index 升序；地址列表按 block/index/hash 降序；都使用 keyset cursor。
- cursor 不暴露数据库内部信息，参数变化时拒绝复用。
- `FINAL` + `is_canonical=true`；ClickHouse 不可用不回退 PostgreSQL。
- uint256 用字符串；`to_address=null` 的创建交易遵循设计文档。
- 返回 `meta.indexed_through_block` 和分页元数据；不把完整 tx 数组嵌入 block detail。

**验收**：

```bash
cargo fmt --check
cargo test --locked --lib
cargo check --locked --features clickhouse
```

使用 axum router tests 覆盖正常、非法地址/hash、404、503、cursor 和 direction。

### T6：sync status、admin 控制和可观测性

**目标**：让运维可以初始化、查看、暂停和恢复数据采集。

**建议文件**：

- 在 block transaction API 中增加 admin-only PUT/POST 控制和 read-only status。
- 修改 dashboard/telemetry 文档或模块，增加 blocks/transactions 指标。
- 更新 `docs/USAGE.md`、`.env.example`、必要的 Compose 环境透传。

**最小能力**：

- 设置/更新 start block、batch size、并发和 reorg window。
- pause/resume，读取 next block、safe head、lag、last error、last success。
- 指标或结构化日志：RPC latency、batch size、写入行数、retry、reorg、ClickHouse error。

**验收**：管理员和 read-only 权限测试；重启后配置和状态保持；开关关闭时无 worker。

### T7：全量验证、文档和交付检查

**目标**：收敛所有变更，确认没有破坏现有 raw-log 系统。

**必须更新**：

- `docs/USAGE.md`：启用、初始化、查询、暂停、容量和故障处理。
- `docs/ARCHITECTURE.md` 或新增链接：block/transaction 数据职责和边界。
- OpenAPI 输出和示例 curl。
- integration test README 或运行说明。

**验收命令**：

```bash
cargo fmt --check
cargo check --locked
cargo test --locked
cargo check --locked --features clickhouse
cargo test --locked --features clickhouse
docker compose -f docker-compose.yml config --quiet
docker compose -f docker-compose.clickhouse.yml config --quiet
git diff --check
```

真实 ClickHouse/PostgreSQL 可用时，额外执行 block/transaction integration suite，并保存
结果、数据量、查询耗时和磁盘占用。当前环境缺少工具或服务时必须明确写出未执行项目。

## 4. Gemini 任务交接格式

每个任务结束后必须输出：

```text
任务：Tn
修改文件：...
实现摘要：...
验证命令及结果：...
已知限制：...
下一任务前置条件：...
```

遇到与现有代码冲突时，先停在当前任务，给出冲突文件、证据和最小解决方案；不要直接
删除用户已有改动，也不要用大规模重构掩盖冲突。
