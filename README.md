# EventLake

EventLake 是一个面向 EVM 兼容区块链的高性能原始事件日志（Raw Event Logs）及区块交易数据采集、索引与检索系统。

## 核心架构原则

- **Raw Event Lake 优先**：专注并保障 EVM 原始日志的完整性、有序性与实时采集，不在此做易变的 ABI 解码，解码解耦至下游消费者。
- **纯净双层极简存储**：
  - **SQLite**：轻量级嵌入式运行在 Rust 进程内，负责订阅定义、区块 Checkpoint、RPC 节点池与认证等控制面事实源，彻底摆脱外部 RDBMS 依赖。
  - **ClickHouse**：系统唯一且强制的高性能分析型原始数据湖，承载全链无过滤（`all_events`）海量日志与区块交易数据的高通量写入与检索。
- **开箱即用统一备份与灾难恢复**：内置支持本地与云端（S3 / MinIO / Cloudflare R2）的 SQLite 原子快照及 ClickHouse 增量备份与恢复工具链。

## 快速开始

```bash
# 1. 准备配置文件
cp .env.example .env

# 2. 启动服务 (ClickHouse + EventLake)
docker compose --env-file .env up -d --build

# 3. 健康检查
curl -fsS http://127.0.0.1:8080/health/ready
```

## 相关文档

- [使用指南 (USAGE)](docs/USAGE.md)
- [Docker 部署手册 (DEPLOYMENT)](docs/DEPLOYMENT.md)
- [备份与灾难恢复指南 (BACKUP_AND_RESTORE)](docs/BACKUP_AND_RESTORE.md)
- [系统架构权威说明 (ARCHITECTURE)](docs/AI/ARCHITECTURE.md)
- [AI Agent 开发规范 (AGENTS)](AGENTS.md)
