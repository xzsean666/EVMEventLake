# EventLake 部署运维指南 (Deployment Guide)

Version: 2.1

Status: Current implementation

## 1. 部署模式全景

EventLake 收敛为极简统一架构：**SQLite**（轻量嵌入式控制面）+ **ClickHouse**（唯一原始事件与区块交易数据湖）。

为彻底解决“服务器端编译耗时漫长、依赖复杂”的痛点，构建流水线已完全移至云端 GitHub Actions。目标服务器支持以下两种零本地编译的生产部署形态：

| 模式 | 运行依赖 | 核心优势 | 推荐场景 |
| :--- | :--- | :--- | :--- |
| **模式 A：预编译 Docker Compose (默认)** | Docker & Compose | **零 Rust 编译**，直接基于随仓库分发的预编译二进制秒级构建容器，一键拉起 ClickHouse。 | 快速容器化部署、生产环境。 |
| **模式 B：预编译独立二进制 (Systemd)** | 无 (或独立跑 ClickHouse) | **零编译、零 Docker 负担**，原生 Linux 二进制秒级启动，Systemd 工业级守护。 | 生产服务器、单机轻量部署、资源紧张的小型 VPS。 |
| **模式 C：本地源码构建 (开发调试)** | Rust 工具链 / Docker build | 基于当前源码重新编译（`docker-compose.source.yml`）。 | 本地二次开发、定制测试。 |

---

## 2. 模式 A：预编译独立二进制部署 (GitHub Release + Systemd) - 最简推荐

### 2.1 一键安装/升级二进制

通过一键脚本直接从 GitHub Release 下载预编译的 Linux x86_64 二进制：

```bash
# 标准下载安装（解压至 /usr/local/bin/eventlake）
curl -sSL https://raw.githubusercontent.com/xzsean666/EVMEventLake/main/scripts/install.sh | bash

# 中国大陆服务器高速下载（启用代理加速）：
curl -sSL https://raw.githubusercontent.com/xzsean666/EVMEventLake/main/scripts/install.sh | bash -s -- --cn

# 指定固定版本（如 v0.1.1）：
curl -sSL https://raw.githubusercontent.com/xzsean666/EVMEventLake/main/scripts/install.sh | bash -s -- --version v0.1.1
```

### 2.2 启动 ClickHouse 依赖

EventLake 依赖 ClickHouse 保存事件湖数据。只需启动一个纯运行时 ClickHouse 单容器（无需编译）：

```bash
mkdir -p /opt/eventlake/data/clickhouse
docker run -d \
  --name clickhouse \
  --restart unless-stopped \
  -p 8123:8123 -p 9000:9000 \
  -e CLICKHOUSE_DB=eventlake \
  -e CLICKHOUSE_USER=eventlake \
  -e CLICKHOUSE_PASSWORD=eventlake \
  -v /opt/eventlake/data/clickhouse:/var/lib/clickhouse \
  clickhouse/clickhouse-server:24.8
```

### 2.3 配置与 Systemd 守护

1. 准备配置工作目录：
```bash
mkdir -p /opt/eventlake/data
cp .env.example /opt/eventlake/.env
# 修改 /opt/eventlake/.env 中的强随机 JWT 密钥及连接串
```

2. 安装 Systemd 服务：
```bash
cp deploy/systemd/eventlake.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now eventlake
```

3. 检查服务与日志：
```bash
# 查看状态
systemctl status eventlake

# 实时日志追踪
journalctl -u eventlake -f

# 验证健康检查端点
curl -fsS http://127.0.0.1:8080/health/ready
```

---

## 3. 模式 A：预编译 Docker Compose 部署 (默认推荐)

克隆仓库后，默认 `docker-compose.yml` 具备**零本地 Rust 编译**与**自适应 Release 拉取**能力：
- 若本地存在 `deploy/prebuilt/eventlake`，秒级复用构建；
- 若本地无二进制（如全新克隆环境），Docker 构建阶段自动从 GitHub Releases 下载官方 Linux 预编译二进制包并解压安装。

### 3.1 启动服务

```bash
# 1. 复制环境变量
cp .env.example .env

# 2. 可选：显式下载指定版本的预编译二进制到本地构建上下文（支持中国大陆代理加速）
./scripts/download-prebuilt-binary.sh --cn

# 3. 一键构建并启动服务（零 Rust 编译，几秒内完成）
docker compose up -d --build

# 4. 检查服务状态
docker compose ps

# 5. 验证就绪
curl -fsS http://127.0.0.1:8080/health/ready
```

### 3.2 常用运维命令

```bash
# 查看实时日志
docker compose logs -f eventlake
docker compose logs -f clickhouse

# 升级到最新发布版本
docker compose pull && docker compose up -d

# 停止服务
docker compose down
```

---

## 4. 模式 C：本地源码构建部署 (开发调试)

如果需要基于当前工作目录源码重新构建镜像进行本地调试：

```bash
docker compose -f docker-compose.source.yml up -d --build
```

---

## 5. 备份与灾难恢复

系统内置全自动本地与 S3 云端备份脚本，详见 [`docs/BACKUP_AND_RESTORE.md`](BACKUP_AND_RESTORE.md)：

```bash
# 全量备份 SQLite + ClickHouse
./scripts/backup.sh --full --local

# 校验备份完整性与数据行数
./scripts/verify-backup.sh --latest
```
