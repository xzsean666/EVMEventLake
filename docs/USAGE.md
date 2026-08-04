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

默认部署只启动 PostgreSQL 和 EventLake。ClickHouse 模式只用于大量事件的搜索：
PostgreSQL 保留 raw log、订阅、队列、认证和 reorg 状态，解码事件及地址/字段索引
只写 ClickHouse，不在两边双写。

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
`--features clickhouse` 构建；ClickHouse 连接失败时服务仍会启动，但搜索会返回服务
错误，解码队列持续重试且受影响订阅暂停，成功后自动恢复。它不会回退 PostgreSQL，
否则必须保留同一份派生数据。

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

### 5.2 添加 JSON-RPC endpoint

```bash
curl -sS -X POST http://127.0.0.1:8080/api/rpc-endpoints \
  -H 'content-type: application/json' \
  -d '{"chain_id":31337,"url":"http://host.docker.internal:8545","weight":100}'
```

可用 `POST /api/rpc-endpoints/{id}/check` 主动检查 endpoint；后台 worker 也会持续
检查并按失败次数、权重和延迟选择 endpoint。

### 5.3 上传 ABI

`abi_json` 可以是标准 Solidity ABI 数组。上传后响应中的 `data.id` 是订阅需要的
`abi_id`。

```bash
curl -sS -X POST http://127.0.0.1:8080/api/abis \
  -H 'content-type: application/json' \
  -d '{
    "name":"MyContract",
    "abi_json":[{
      "type":"event",
      "name":"Transfer",
      "anonymous":false,
      "inputs":[
        {"name":"from","type":"address","indexed":true},
        {"name":"to","type":"address","indexed":true},
        {"name":"value","type":"uint256","indexed":false}
      ]
    }]
  }'
```

### 5.4 创建订阅

```bash
curl -sS -X POST http://127.0.0.1:8080/api/subscriptions \
  -H 'content-type: application/json' \
  -d '{
    "chain_id":31337,
    "contract_address":"0x0000000000000000000000000000000000000001",
    "abi_id":"REPLACE_WITH_ABI_UUID",
    "start_block":0,
    "realtime_enabled":true
  }'
```

同一 `chain_id + contract_address` 只能有一个 active subscription。启动后台 worker
后，流程为：RPC 拉取 raw logs -> PostgreSQL 保存 raw log 和 decode queue -> ABI 解码
-> 根据启动模式写入 PostgreSQL 或 ClickHouse 的解码事件和地址/字段索引。ClickHouse
写入未成功前 queue 不会确认，订阅会重试并暂停采集。

## 6. 搜索和探索

`POST /api/search` 支持字段 `chain_id`、`block_number`、`contract_address`、
`event_name`、`topic0`、`transaction_hash`、`address` 和 `field.<name>`。过滤器在
当前实现中按 AND 组合。

```bash
curl -sS -X POST http://127.0.0.1:8080/api/search \
  -H 'content-type: application/json' \
  -d '{
    "page":1,
    "limit":50,
    "filters":[
      {"field":"chain_id","operator":"eq","value":31337},
      {"field":"event_name","operator":"eq","value":"Transfer"},
      {"field":"field.value","operator":"eq","value":"1000"}
    ],
    "sort":{"field":"block_number","direction":"desc"}
  }'
```

探索接口：

- `GET /api/explorer/address/{address}`
- `GET /api/explorer/contracts/{chain_id}/{contract_address}`
- `GET /api/explorer/events/{event_name}`
- `GET /api/dashboard`

ClickHouse 模式读取 `decoded_events FINAL` 及其索引；PostgreSQL-only 模式读取
PostgreSQL 派生表。前者不可用时不会回退到 PostgreSQL，以免结果不完整或重新引入双份
派生数据。

已有 PostgreSQL-only 历史数据时，先完成从 retained raw logs 的受控回填和核验，再切换
到 ClickHouse 模式；当前版本不会自动迁移或删除旧的 PostgreSQL 派生表。

## 7. 配置和日志

运行时配置集中在 `.env`。常用键：

| 变量 | 默认值 | 作用 |
| --- | --- | --- |
| `EVENTLAKE_HTTP_HOST` | `0.0.0.0`（Compose） | HTTP 监听地址 |
| `EVENTLAKE_HTTP_PORT` | `8080` | HTTP 监听端口 |
| `EVENTLAKE_DATABASE_URL` | Compose 内部 PostgreSQL URL | PostgreSQL 连接 |
| `EVENTLAKE_BACKGROUND_WORKERS_ENABLED` | `true` | 启用采集、解码和维护 worker |
| `EVENTLAKE_WORKER_TICK_SECONDS` | `5` | worker 调度间隔 |
| `EVENTLAKE_DECODE_BATCH_SIZE` | `100` | 每次解码批量大小 |
| `EVENTLAKE_CLICKHOUSE_ENABLED` | `false` | 启用 ClickHouse（需 feature 构建） |
| `EVENTLAKE_LOG_LEVEL` | `info` | tracing 过滤级别 |

查看日志：

```bash
docker compose --env-file .env logs -f eventlake
docker compose --env-file .env -f docker-compose.clickhouse.yml logs -f clickhouse
```

## 8. 开发验证

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
