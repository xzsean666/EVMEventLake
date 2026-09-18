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
- `Dockerfile.prebuilt`
- `Dockerfile.prebuilt.cn`
- `docker-compose.yml`
- `docker-compose.prebuilt.yml`
- `docker-compose.prebuilt.cn.yml`
- `scripts/build-prebuilt-binary.sh`
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
- `docker compose --env-file .env.example -f docker-compose.prebuilt.yml config`
- `docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config`

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

## 5. Development Workflow

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

Run with Docker Compose:

```bash
docker compose up --build
```

Build the prebuilt binary:

```bash
scripts/build-prebuilt-binary.sh
```

Run the prebuilt binary container:

```bash
docker compose -f docker-compose.prebuilt.yml up -d --build
```

## 6. Database Migrations

The implementation uses embedded SQLx migrations for SQLite schema setup.
Migrations are compiled into the binary via `sqlx::migrate!("./migrations")` and executed automatically at startup.

## 7. Health Checks

- `/health/live`: Basic liveness check.
- `/health/ready`: Readiness check verifying SQLite connectivity.
