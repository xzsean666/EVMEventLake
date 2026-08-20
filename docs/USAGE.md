# EventLake 使用说明

本文对应当前仓库中的可运行实现。架构和 ClickHouse 数据一致性说明分别见
[`ARCHITECTURE.md`](ARCHITECTURE.md) 和 [`CLICKHOUSE_UPGRADE.md`](CLICKHOUSE_UPGRADE.md)。

## 1. 选择部署方式

| 目的 | Compose 文件 |
| --- | --- |
| PostgreSQL-only，本地开发或小规模部署 | `docker-compose.yml` |
| ClickHouse 分析查询，源码构建 | `docker-compose.clickhouse.yml` |
| 已构建二进制 | `docker-compose.prebuilt.yml` |
| 已构建 ClickHouse 二进制 | `docker-compose.prebuilt.clickhouse.yml` |
| 中国大陆网络环境 | 对应的 `.cn.yml` 文件 |

默认部署只启动 PostgreSQL 和 EventLake。PostgreSQL-only 模式把 raw log 存在
PostgreSQL；ClickHouse 模式把 raw log 只存到 ClickHouse，PostgreSQL 只保留订阅、
checkpoint、认证和 reorg 状态。这个项目不再启动 ABI decoder，所有订阅的 `topics` 和
`data` 都由下游项目解码。

## 2. 启动本地服务

要求：Docker、Docker Compose；源码构建还需要 Rust 1.94 及 Cargo。

```bash
cp .env.example .env
docker compose --env-file .env up -d --build
docker compose --env-file .env ps
curl -fsS http://127.0.0.1:8080/health/ready
```

首次启动会自动执行 `migrations/` 中的 PostgreSQL migration。停止服务：

```bash
docker compose --env-file .env down
```

只在可丢弃的本地环境中删除 PostgreSQL 数据卷：

```bash
docker compose --env-file .env down -v
```

### 启用 ClickHouse

源码构建和运行：

```bash
docker compose --env-file .env -f docker-compose.clickhouse.yml up -d --build
curl -fsS http://127.0.0.1:8080/health/ready
```

该 Compose 文件使用 `clickhouse/clickhouse-server:24.8`，通过 HTTP `8123` 连接，
并把数据和日志分别保存到 `data/clickhouse`、`logs/clickhouse`。应用镜像使用
`--features clickhouse` 构建；ClickHouse 连接失败时服务仍会启动，但 raw-log 写入和
查询会等待 ClickHouse 恢复，不会回退 PostgreSQL，也不会推进失败区间的 checkpoint。

预编译部署：

```bash
EVENTLAKE_PREBUILT_BINARY=deploy/prebuilt/eventlake-clickhouse \
  scripts/build-prebuilt-binary.sh
docker compose --env-file .env -f docker-compose.prebuilt.clickhouse.yml up -d --build
```

更多镜像、国内镜像和 SSH 隧道选项见 [`DEPLOYMENT.md`](DEPLOYMENT.md)。

## 3. 检查服务和查看 API

```bash
curl http://127.0.0.1:8080/health/live
curl http://127.0.0.1:8080/health/ready
curl http://127.0.0.1:8080/api/openapi.json > openapi.json
```

所有 API 成功响应使用如下结构：

```json
{"success":true,"data":{},"error":null,"meta":null}
```

## 4. 认证

`.env.example` 默认设置 `EVENTLAKE_REQUIRE_AUTHENTICATION=false`，适合本地开发；
此时每个请求都按 Admin 处理。生产环境必须设置强随机
`EVENTLAKE_JWT_SECRET` 并启用 `EVENTLAKE_REQUIRE_AUTHENTICATION=true`。

启用认证后支持两种凭据：

- API key：请求头 `X-API-Key: evl_...`。
- JWT：请求头 `Authorization: Bearer <token>`，使用 HS256，claims 需要 `sub`、`role`
  （`admin` 或 `read_only`）和 `exp`。

API key 只在创建响应中返回一次。建议先在本地认证关闭时创建 Admin key，安全保存
该 key，再重启并开启认证：

```bash
curl -sS -X POST http://127.0.0.1:8080/api/auth/api-keys \
  -H 'content-type: application/json' \
  -d '{"name":"operator","role":"admin"}'

curl -sS http://127.0.0.1:8080/api/chains \
  -H 'X-API-Key: evl_REPLACE_WITH_RETURNED_KEY'
```

写入、暂停/恢复订阅和管理 API key 需要 Admin；查询接口可使用 ReadOnly。

## 5. 第一个索引任务

以下顺序适用于默认预置链，也适用于通过 API 新增的链。

### 5.1 添加或确认链

```bash
curl -sS -X POST http://127.0.0.1:8080/api/chains \
  -H 'content-type: application/json' \
  -d '{
    "chain_id":31337,
    "name":"Local Anvil",
    "native_token_symbol":"ETH",
    "safe_confirmation_depth":0,
    "default_max_block_window":100
  }'
```

### 5.2 配置与管理 JSON-RPC endpoints

#### 方式 A：通过种子 JSON 文件启动时自动导入（推荐）
可以在 `config/rpc_endpoints.json` 中配置默认内置的 RPC 节点（参考 [`config/rpc_endpoints.json.example`](../config/rpc_endpoints.json.example)），服务启动时会自动读取并幂等写入数据库（已存在的节点不会重复插入或覆盖）：

```json
[
  {
    "chain_id": 1,
    "url": "https://eth.llamarpc.com",
    "weight": 100
  },
  {
    "chain_id": 8453,
    "url": "https://mainnet.base.org",
    "weight": 100
  }
]
```

环境变量 `EVENTLAKE_RPC_SEEDS_PATH=config/rpc_endpoints.json` 可自定义种子文件路径。
如果你需要借助 AI 快速生成涵盖所有主流公链的最新公开免费 RPC 列表，可直接使用提示词模板 [`docs/RPC_SEEDS_PROMPT.md`](RPC_SEEDS_PROMPT.md)。

#### 方式 B：通过 API 动态添加、删除或停用

- **添加新端点**：
```bash
curl -sS -X POST http://127.0.0.1:8080/api/rpc-endpoints \
  -H 'content-type: application/json' \
  -d '{"chain_id":31337,"url":"http://host.docker.internal:8545","weight":100}'
```

- **删除端点**：
```bash
curl -sS -X DELETE http://127.0.0.1:8080/api/rpc-endpoints/{id}
```

- **启用 / 禁用端点**：
```bash
curl -sS -X POST http://127.0.0.1:8080/api/rpc-endpoints/{id}/disable
curl -sS -X POST http://127.0.0.1:8080/api/rpc-endpoints/{id}/enable
```

可用 `POST /api/rpc-endpoints/{id}/check` 主动检查 endpoint；后台 worker 也会持续
检查并按失败次数、权重和延迟选择 endpoint。

### 5.3 创建合约订阅

```bash
curl -sS -X POST http://127.0.0.1:8080/api/subscriptions \
  -H 'content-type: application/json' \
  -d '{
    "chain_id":31337,
    "contract_address":"0x0000000000000000000000000000000000000001",
    "start_block":0,
    "realtime_enabled":true
  }'
```

省略 `collection_scope` 时默认为 `contract`。同一
`chain_id + collection_scope + contract_address` 只能有一个 active subscription。启动
后台 worker 后，流程为：RPC 拉取 raw logs -> 写入当前 raw store -> 推进 PostgreSQL
checkpoint。`start_block` 是必填的起始区块，第一次请求范围从该区块开始（包含该区块），
完成后 checkpoint 推进到下一块；项目不会解码 topics 或 data。
为避免全量和按合约重复拉取，同一链不能同时存在 active 的 `all_events` 与
`contract` subscriptions。

如果只想按合约地址收集该合约的全部 raw logs，不需要 ABI，可以批量创建：

~~~bash
curl -sS -X POST http://127.0.0.1:8080/api/subscriptions/batch \
  -H 'content-type: application/json' \
  -d '{
    "chain_id":31337,
    "contract_addresses":[
      "0x4444444444444444444444444444444444444444",
      "0x5555555555555555555555555555555555555555"
    ],
    "start_block":100,
    "realtime_enabled":true
  }'
~~~

批量接口会规范化并去重输入地址；同一链同一地址已有 active subscription 时返回已有记录，
不会重新创建或重复采集。abi_id 可以省略。

### 5.4 创建全量事件订阅

`EVENTLAKE_CLICKHOUSE_ENABLED=true` 只是启用 ClickHouse raw store，并不会自动创建全量
订阅。全量模式需要该开关且二进制必须以 `clickhouse` feature 构建；创建订阅时设置
`collection_scope: "all_events"`，不传 `contract_address`，RPC `eth_getLogs` 才会不带
地址过滤：

```bash
curl -sS -X POST http://127.0.0.1:8080/api/subscriptions \
  -H 'content-type: application/json' \
  -d '{
    "chain_id":31337,
    "collection_scope":"all_events",
    "start_block":0,
    "realtime_enabled":true,
    "min_block_window":1,
    "max_block_window":100
  }'
```

`all_events` 与合约订阅一样使用 `start_block`，从指定区块开始收集整条链的所有日志。
`abi_id` 在当前 raw-event-lake 模式仅为兼容字段，不会触发本项目解码。

## 6. 搜索和探索

主搜索接口为 `POST /api/raw-logs/search`。它要求 `chain_id` 的 `eq` 过滤器，并支持
`block_number`、`contract_address`、`transaction_hash`、`topic0` 到 `topic3`；过滤器按
AND 组合。返回 raw `topics` 和 `data`，不包含 ABI 解码字段。

```bash
curl -sS -X POST http://127.0.0.1:8080/api/raw-logs/search \
  -H 'content-type: application/json' \
  -d '{
    "page":1,
    "limit":50,
    "filters":[
      {"field":"chain_id","operator":"eq","value":31337},
      {"field":"block_number","operator":"gte","value":100},
      {"field":"topic0","operator":"eq","value":"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"}
    ],
    "sort":{"field":"block_number","direction":"desc"}
  }'
```

ClickHouse 模式查询 `raw_logs FINAL`；PostgreSQL-only 模式查询
`eventlake_raw_logs`。ClickHouse 不可用时不会回退 PostgreSQL，避免返回不完整数据。
`/api/search` 和 explorer 接口保留为历史 decoded 数据兼容读取面，新采集不会填充它们。

## 7. 区块与交易数据同步与查询 (Block & Transaction)

当开启 `EVENTLAKE_BLOCK_TRANSACTION_ENABLED=true` 且启用了 ClickHouse 时，EventLake 支持整链的标准 EVM 区块和交易全量抓取与高性能查询。

### 7.1 配置整链区块交易同步任务

```bash
curl -sS -X PUT http://127.0.0.1:8080/api/chains/31337/block-transaction-sync \
  -H 'content-type: application/json' \
  -d '{
    "start_block": 0,
    "end_block": null,
    "batch_size": 10,
    "reorg_window": 32,
    "realtime_enabled": true,
    "status": "pending"
  }'
```

### 7.2 查看同步状态与控制

查看同步状态：
```bash
curl -sS http://127.0.0.1:8080/api/chains/31337/sync-status
```

暂停与恢复同步：
```bash
curl -sS -X POST http://127.0.0.1:8080/api/chains/31337/block-transaction-sync/pause
curl -sS -X POST http://127.0.0.1:8080/api/chains/31337/block-transaction-sync/resume
```

### 7.3 查询区块与交易 API

1. **查询区块详情**（支持十进制高度、十六进制高度或 32 字节 Hash）：
```bash
curl -sS http://127.0.0.1:8080/api/chains/31337/blocks/123456
curl -sS http://127.0.0.1:8080/api/chains/31337/blocks/0x1e240
curl -sS http://127.0.0.1:8080/api/chains/31337/blocks/0x000000000000000000000000000000000000000000000000000000000001e240
```

2. **分页查询区块内的交易列表**：
```bash
curl -sS "http://127.0.0.1:8080/api/chains/31337/blocks/123456/transactions?limit=100"
```

3. **查询交易详情**：
```bash
curl -sS http://127.0.0.1:8080/api/chains/31337/transactions/0x00000000000000000000000000000000000000000000000000000000000000a1
```

4. **查询地址交易列表（支持 keyset cursor 分页与方向过滤）**：
```bash
curl -sS "http://127.0.0.1:8080/api/chains/31337/addresses/0x1111111111111111111111111111111111111111/transactions?direction=any&limit=50"
```

## 8. 配置和日志

运行时配置集中在 `.env`。常用键：

| 变量 | 默认值 | 作用 |
| --- | --- | --- |
| `EVENTLAKE_HTTP_HOST` | `0.0.0.0`（Compose） | HTTP 监听地址 |
| `EVENTLAKE_HTTP_PORT` | `8080` | HTTP 监听端口 |
| `EVENTLAKE_DATABASE_URL` | Compose 内部 PostgreSQL URL | PostgreSQL 连接 |
| `EVENTLAKE_BACKGROUND_WORKERS_ENABLED` | `true` | 启用采集、RPC 检查和维护 worker |
| `EVENTLAKE_WORKER_TICK_SECONDS` | `5` | worker 调度间隔 |
| `EVENTLAKE_CLICKHOUSE_ENABLED` | `false` | 启用 ClickHouse raw store（需 feature 构建） |
| `EVENTLAKE_BLOCK_TRANSACTION_ENABLED` | `false` | 启用整链区块和交易后台同步 worker |
| `EVENTLAKE_BLOCK_TRANSACTION_BATCH_SIZE` | `10` | 区块与交易批量拉取区块数量 |
| `EVENTLAKE_BLOCK_TRANSACTION_MAX_CONCURRENCY` | `2` | 最大并发同步链数量 |
| `EVENTLAKE_BLOCK_TRANSACTION_REORG_WINDOW` | `32` | 区块分叉检测回退窗口 |
| `EVENTLAKE_BLOCK_TRANSACTION_MAX_RESPONSE_BYTES` | `67108864` | 单次 RPC 响应最大字节数上限 |
| `EVENTLAKE_RPC_SEEDS_PATH` | `config/rpc_endpoints.json` | 启动时自动导入 RPC 端点种子 JSON 文件路径 |
| `EVENTLAKE_LOG_LEVEL` | `info` | tracing 过滤级别 |

查看日志：

```bash
docker compose --env-file .env logs -f eventlake
docker compose --env-file .env -f docker-compose.clickhouse.yml logs -f clickhouse
```

## 9. 开发验证

```bash
cargo fmt --check
cargo check --locked
cargo test --locked
cargo check --locked --features clickhouse
cargo test --locked --features clickhouse
```

ClickHouse 集成测试默认跳过真实服务；已启动 ClickHouse 后可显式运行：

```bash
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true \
  cargo test --locked --features clickhouse --test clickhouse_integration_tests -- --nocapture
```

完整部署矩阵和 Compose 配置检查见 [`DEPLOYMENT.md`](DEPLOYMENT.md)。

