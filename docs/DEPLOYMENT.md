# EventLake Docker Deployment

Version: 2.0

Status: Current implementation

## 1. Deployment Modes

EventLake 收敛为极简统一架构：**SQLite**（轻量嵌入式控制面）+ **ClickHouse**（唯一原始事件与区块交易数据湖）。

系统提供三套标准 Docker 部署方式：

| Mode | Files | When to use |
| --- | --- | --- |
| **源码构建 (Source build)** | `Dockerfile`, `docker-compose.yml` | 本地完整开发、CI 流水线或具备 Rust 编译环境的主机。 |
| **预编译二进制部署 (Prebuilt binary)** | `scripts/build-prebuilt-binary.sh`, `Dockerfile.prebuilt`, `docker-compose.prebuilt.yml` | 生产服务器或只需要打包并运行 Linux release 二进制的环境。 |
| **中国大陆镜像预编译 (China prebuilt)** | `Dockerfile.prebuilt.cn`, `docker-compose.prebuilt.cn.yml` | 中国大陆网络环境（自动配置 DaoCloud Docker 镜像加速与 Aliyun apt 镜像源）。 |

每套部署统一启动两个服务：
- `clickhouse`: 高性能列式数据湖引擎（`clickhouse/clickhouse-server:24.8`），对外提供 HTTP（8123）与 Native（9000）接口，数据持久化于 `./data/clickhouse`。
- `eventlake`: Rust 单体核心进程，内嵌 SQLite 作为元数据事实源（持久化于 `./data/sqlite`）。

---

## 2. 运行时配置与目录约定

- Rust package: `eventlake`
- 二进制产物: `eventlake`（位于 `/usr/local/bin/eventlake` 或 `deploy/prebuilt/eventlake`）
- 容器用户: `eventlake` (uid 10001)
- 监听端口: `8080`（容器内），可通过环境变量 `EVENTLAKE_HTTP_PORT` 映射到宿主机
- 健康检查: `/health/ready`（检查 HTTP 服务及底层存储连通性）
- 持久化目录结构：
  - `./data/sqlite`: SQLite 数据库文件（`eventlake.db`）
  - `./data/clickhouse`: ClickHouse 数据分片与元数据
  - `./logs/clickhouse`: ClickHouse 服务日志
  - `./backups`: 本地备份归档目录
- 统一备份与灾难恢复：详见 [`docs/BACKUP_AND_RESTORE.md`](BACKUP_AND_RESTORE.md)。

---

## 3. 环境配置文件 (.env)

从模版复制配置文件：

```bash
cp .env.example .env
```

在生产环境中，请至少修改：
- `EVENTLAKE_JWT_SECRET`：强随机密钥
- `EVENTLAKE_DATABASE_URL`：SQLite 连接串（Compose 默认为 `sqlite:///data/eventlake.db?mode=rwc`）
- `EVENTLAKE_CLICKHOUSE_URL`：ClickHouse 连接串（Compose 默认为 `http://eventlake:eventlake@clickhouse:8123/eventlake`）

如果使用自定义环境变量文件，同时传递给 Compose 与容器：

```bash
EVENTLAKE_ENV_FILE=.env.prod docker compose --env-file .env.prod up -d --build
```

---

## 4. 模式一：源码构建部署 (Source Build)

适用于具备外网或本地包含 Rust 依赖缓存的环境：

```bash
# 启动 ClickHouse 与 EventLake
docker compose --env-file .env up -d --build

# 检查服务运行状态
docker compose --env-file .env ps

# 验证就绪状态
curl -fsS http://127.0.0.1:8080/health/ready
```

停止服务：

```bash
docker compose --env-file .env down
```

---

## 5. 模式二：预编译二进制部署 (Prebuilt Binary)

1. 先在构建机或宿主机编译 Linux release 二进制：

```bash
scripts/build-prebuilt-binary.sh
```

生成的二进制保存在 `deploy/prebuilt/eventlake`。

2. 在目标主机启动精简运行时镜像（Debian slim）：

```bash
docker compose --env-file .env -f docker-compose.prebuilt.yml up -d --build
docker compose --env-file .env -f docker-compose.prebuilt.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

可选参数：
| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `EVENTLAKE_PREBUILT_BINARY` | `deploy/prebuilt/eventlake` | 指定预编译二进制路径 |
| `EVENTLAKE_CARGO_TARGET` | 宿主机架构 | 交叉编译 target triple（例如 `x86_64-unknown-linux-gnu`） |
| `CARGO_TARGET_DIR` | `target` | Cargo 构建目录 |

---

## 6. 模式三：中国大陆加速预编译部署 (China Prebuilt)

1. 编译 Linux 二进制：

```bash
scripts/build-prebuilt-binary.sh
```

2. 使用配置了国内镜像源的 Compose 启动：

```bash
docker compose --env-file .env -f docker-compose.prebuilt.cn.yml up -d --build
docker compose --env-file .env -f docker-compose.prebuilt.cn.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

支持覆盖的国内源环境变量：
| 变量 | 默认配置 | 说明 |
| --- | --- | --- |
| `EVENTLAKE_DEBIAN_IMAGE` | `m.daocloud.io/docker.io/library/debian:bookworm-slim` | Debian 基础镜像 |
| `EVENTLAKE_DEBIAN_MIRROR` | `http://mirrors.aliyun.com/debian` | 阿里云 Debian 镜像源 |
| `EVENTLAKE_DEBIAN_SECURITY_MIRROR` | `http://mirrors.aliyun.com/debian-security` | 阿里云 Debian 安全更新源 |
| `EVENTLAKE_CLICKHOUSE_IMAGE` | `m.daocloud.io/docker.io/clickhouse/clickhouse-server:24.8` | ClickHouse 镜像代理 |

---

## 7. 运维与验证

### 7.1 Compose 静态语法检查

```bash
docker compose --env-file .env.example -f docker-compose.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config > /dev/null
```

### 7.2 日志查看

```bash
# 查看 EventLake 实时日志
docker compose logs -f eventlake

# 查看 ClickHouse 实时日志
docker compose logs -f clickhouse
```

### 7.3 一键备份与恢复

系统内置全自动本地与云端备份脚本，详见 [`docs/BACKUP_AND_RESTORE.md`](BACKUP_AND_RESTORE.md)：

```bash
# 全量备份
./scripts/backup.sh --full --local

# 备份完整性验证
./scripts/verify-backup.sh --latest
```
