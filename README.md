# EventLake

EventLake 是一个 EVM 原始事件采集和搜索服务，不在本项目内做 ABI 解码。默认使用
PostgreSQL 保存 raw log；启用 `clickhouse` feature 和对应 Compose 文件后，raw log 仅写入
ClickHouse，PostgreSQL 保留订阅、checkpoint、RPC 和认证等运行状态。下游项目可以按自己
的 ABI 和版本策略解码 `topics` 与 `data`。

快速开始：

```bash
cp .env.example .env
docker compose --env-file .env up -d --build
curl -fsS http://127.0.0.1:8080/health/ready
```

完整使用流程见 [`docs/USAGE.md`](docs/USAGE.md)，部署说明见 [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md)，系统架构与开发规范见 [`docs/AI/ARCHITECTURE.md`](docs/AI/ARCHITECTURE.md) 与 [`AGENTS.md`](AGENTS.md)。
