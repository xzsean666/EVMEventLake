# EventLake Build and Usage Guide

Version: 2.0

Status: Current implementation

## 1. Current Stage

The repository contains the EVMEventLake Rust monolith implementation with a single unified storage architecture:
- **SQLite**: Embedded metadata and control plane store (subscriptions, checkpoints, RPC pool, chains, auth).
- **ClickHouse**: Analytical raw event lake and block/transaction store.

For an end-user quick start and API workflow, see [`USAGE.md`](USAGE.md).
For deployment specifications, see [`DEPLOYMENT.md`](DEPLOYMENT.md).
For backup and disaster recovery, see [`BACKUP_AND_RESTORE.md`](BACKUP_AND_RESTORE.md).

Core files include:

- `Cargo.toml` & `Cargo.lock`
- `src/`
- `clickhouse/`
- `migrations/`
- `Dockerfile`
- `docker-compose.yml`
- `docker-compose.source.yml`
- `scripts/build-prebuilt-binary.sh`
- `scripts/download-prebuilt-binary.sh`
- `scripts/backup.sh`
- `scripts/restore.sh`
- `scripts/verify-backup.sh`
- `scripts/s3-helper.py`
- `deploy/prebuilt/README.md`

Current verified commands:

- `cargo check --ignore-rust-version --all-targets`
- `cargo build --release --locked`
- `cargo test --ignore-rust-version`
- `cargo test --ignore-rust-version --test e2e_real_database_tests -- --nocapture`
- `scripts/build-prebuilt-binary.sh`
- `tests/test_backup_restore_e2e.sh`
- `docker compose --env-file .env.example config`
- `docker compose --env-file .env.example -f docker-compose.source.yml config`

## 2. Local Development Requirements

Local development assumes:

- Rust stable toolchain (1.94+).
- Cargo.
- Docker & Docker Compose (for ClickHouse and container deployment).
- Python 3 and curl (for backup and S3 helper tools).

Current Rust stack:

- `tokio 1.52.3` for async runtime.
- `axum 0.8.9` for HTTP API.
- `sqlx 0.9.0` with `sqlite` engine for embedded transactional metadata.
- `clickhouse 0.13.3` for high-throughput raw event, block, and transaction data lake.
- `alloy-primitives 1.6.0` for EVM primitive types.
- `reqwest 0.13.4` for JSON-RPC HTTP transport.
- `serde 1.0.228` for serialization.
- `utoipa 5.5.0` for OpenAPI scaffolding.
- `tracing 0.1.43` for structured telemetry.

## 3. Environment Variables

All configuration is centralized in the `configuration` module (`src/configuration/mod.rs`).

Standard variables:

```text
EVENTLAKE_HTTP_HOST=0.0.0.0
EVENTLAKE_HTTP_PORT=8080
EVENTLAKE_DATABASE_URL=sqlite:///data/eventlake.db?mode=rwc
EVENTLAKE_CLICKHOUSE_URL=http://eventlake:eventlake@clickhouse:8123/eventlake
EVENTLAKE_CLICKHOUSE_ENABLED=true
EVENTLAKE_JWT_SECRET=change-me
EVENTLAKE_LOG_LEVEL=info
EVENTLAKE_DEFAULT_PAGE_LIMIT=50
EVENTLAKE_MAX_PAGE_LIMIT=500
EVENTLAKE_REQUIRE_AUTHENTICATION=false
EVENTLAKE_BACKGROUND_WORKERS_ENABLED=true
EVENTLAKE_WORKER_TICK_SECONDS=5
EVENTLAKE_BLOCK_TRANSACTION_ENABLED=false
EVENTLAKE_RPC_SEEDS_PATH=config/rpc_endpoints.json
```

## 4. Docker Deployment Services

The standard deployment consists of:

- `clickhouse`: Official `clickhouse/clickhouse-server:24.8` for analytical raw event storage.
- `eventlake`: Core ingestion and search daemon with embedded SQLite volume mount (`./data/sqlite:/data`).

## 5. CI/CD 与自动化版本发布 (Automated Release Workflow)

为彻底解决生产服务器本地编译慢的痛点，项目配置了云端 GitHub Actions 手动触发流水线 (`.github/workflows/release.yml`)：

### 5.1 触发机制
- **按需手动触发 (Manual Workflow Dispatch)**：不会在日常每次 push 代码时浪费算力构建。
- **自动计算版本号**：支持选择 `patch`（如 0.1.0 -> 0.1.1）、`minor`（如 0.1.0 -> 0.2.0）、`major`（如 0.1.0 -> 1.0.0）或 `custom`（自定义版本）。
- **自动化操作**：
  1. 自动更新 `Cargo.toml` 与 `Cargo.lock`。
  2. 自动打 Git Tag（如 `v0.1.1`）并提交推送。
  3. 云端编译 Linux x86_64 release 二进制包，附带 SHA256 校验和发布至 GitHub Releases。
  4. 自动构建轻量 Docker 镜像，推送至 GHCR (`ghcr.io/xzsean666/evmeventlake:latest` 与 `vX.Y.Z`)。

### 5.2 触发命令

在 GitHub 网页界面进入 **Actions** -> 选择 **Release and Build Prebuilt** -> 点击 **Run workflow**。

或者使用 GitHub CLI 直接在终端触发：

```bash
# 默认 patch 递增 (0.1.0 -> 0.1.1)
gh workflow run release.yml -f bump_type=patch

# 次版本递增 (0.1.0 -> 0.2.0)
gh workflow run release.yml -f bump_type=minor

# 自定义指定版本
gh workflow run release.yml -f bump_type=custom -f custom_version=0.3.0
```

## 6. Development Workflow

Run all tests:

```bash
cargo test --ignore-rust-version
```

Run SQLite E2E integration test:

```bash
cargo test --ignore-rust-version --test e2e_real_database_tests -- --nocapture
```

Run formatting and clippy:

```bash
cargo fmt --check
cargo clippy --ignore-rust-version --all-targets -- -D warnings
```

Run local service:

```bash
cargo run
```

Run with Docker Compose (预编译二进制秒级构建):

```bash
docker compose up -d --build
```

从本地源码调试构建容器:

```bash
docker compose -f docker-compose.source.yml up -d --build
```

## 6. Database Migrations

The implementation uses embedded SQLx migrations for SQLite schema setup.
Migrations are compiled into the binary via `sqlx::migrate!("./migrations")` and executed automatically at startup.

## 7. Health Checks

- `/health/live`: Basic liveness check.
- `/health/ready`: Readiness check verifying SQLite connectivity.
