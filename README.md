# EventLake

EventLake 是一个 EVM 事件采集、解码、索引和搜索服务。默认使用 PostgreSQL；启用
`clickhouse` feature 和对应 Compose 文件后，可在大数据量下把解码事件和搜索索引存入
ClickHouse；PostgreSQL 仍保存 raw log 和运行状态，不会双写同一份派生搜索数据。

快速开始：

```bash
cp .env.example .env
docker compose --env-file .env up -d --build
curl -fsS http://127.0.0.1:8080/health/ready
```

完整使用流程见 [`docs/USAGE.md`](docs/USAGE.md)，部署矩阵见 [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md)，
架构见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。
