# EventLake Build and Usage Guide

Version: 1.1

Status: Current implementation

## 1. Current Stage

The repository contains the Step 4 Rust monolith implementation.

For an end-user quick start and API workflow, see [`USAGE.md`](USAGE.md).

The first Rust monolith implementation has been created.

Created implementation files include:

- `Cargo.toml`
- `src/`
- `migrations/`
- `Dockerfile`
- `Dockerfile.prebuilt`
- `Dockerfile.prebuilt.cn`
- `docker-compose.yml`
- `docker-compose.prebuilt.yml`
- `docker-compose.prebuilt.cn.yml`
- `scripts/build-prebuilt-binary.sh`
- `deploy/prebuilt/README.md`
- `docs/DEPLOYMENT.md`

Current verified commands:

- `cargo check`
- `cargo build --release --locked`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --test e2e_real_database_tests -- --nocapture`
- `scripts/build-prebuilt-binary.sh`
- `docker compose --env-file .env.example config`
- `docker compose --env-file .env.example -f docker-compose.prebuilt.yml config`
- `docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config`

## 2. Expected Local Requirements

Local development assumes:

- Rust stable toolchain.
- Cargo.
- Docker.
- Docker Compose.
- PostgreSQL through Docker Compose for local development.

Current Rust stack:

- `tokio 1.52.3` for async runtime.
- `axum 0.8.9` for HTTP API.
- `sqlx 0.9.0` for PostgreSQL access and migrations.
- `alloy-primitives 1.6.0` for EVM primitive types.
- `alloy-json-abi 1.6.0` for ABI parsing and Event Registry generation.
- `alloy-dyn-abi 1.6.0` for runtime event decoding.
- `reqwest 0.13.4` for JSON-RPC HTTP transport.
- `serde 1.0.228` for serialization.
- `utoipa 5.5.0` for OpenAPI scaffolding.
- `tracing 0.1.43` for structured telemetry.

## 3. Expected Environment Variables

All configuration is centralized in the `configuration` module.

Implemented variables include:

```text
EVENTLAKE_HTTP_HOST=0.0.0.0
EVENTLAKE_HTTP_PORT=8080
EVENTLAKE_DATABASE_URL=postgres://eventlake:eventlake@postgres:5432/eventlake
EVENTLAKE_JWT_SECRET=change-me
EVENTLAKE_LOG_LEVEL=info
EVENTLAKE_DEFAULT_PAGE_LIMIT=50
EVENTLAKE_MAX_PAGE_LIMIT=500
EVENTLAKE_REQUIRE_AUTHENTICATION=false
EVENTLAKE_BACKGROUND_WORKERS_ENABLED=true
EVENTLAKE_WORKER_TICK_SECONDS=5
EVENTLAKE_DECODE_BATCH_SIZE=100
EVENTLAKE_CLICKHOUSE_ENABLED=false
EVENTLAKE_CLICKHOUSE_HOST=clickhouse
EVENTLAKE_CLICKHOUSE_PORT=8123
EVENTLAKE_CLICKHOUSE_USER=eventlake
EVENTLAKE_CLICKHOUSE_PASSWORD=eventlake
EVENTLAKE_CLICKHOUSE_DB=eventlake
```

RPC endpoints should be stored and managed through the database, not hard-coded environment variables.

## 4. Expected Docker Services

The default deployment uses:

- `postgres`
- `eventlake`

The ClickHouse Compose variants add an optional `clickhouse` service and compile
the feature-gated derived-event search store. PostgreSQL remains the source of truth
for raw logs and operational state; the decoded event and search indexes are stored
in exactly one selected search store.

## 5. Expected Development Commands

Run tests:

```text
cargo test
```

Run only the real PostgreSQL E2E test:

```text
cargo test --test e2e_real_database_tests -- --nocapture
```

The E2E test reads `.env.test` and expects:

```text
DATABASE_URL=postgres://...
```

Important:

- `.env.test` is intentionally ignored by Git.
- The E2E test resets only EventLake-owned database objects.
- EventLake-owned business tables use the `eventlake_` prefix.
- EventLake uses `eventlake_sqlx_migrations` instead of the default `_sqlx_migrations` table.
- Use a dedicated disposable test database only.

Run formatting:

```text
cargo fmt
```

Run linting:

```text
cargo clippy --all-targets --all-features -- -D warnings
```

Run local service:

```text
cargo run
```

Run with Docker Compose:

```text
docker compose up --build
```

Run with Docker Compose and an explicit env file:

```text
EVENTLAKE_ENV_FILE=.env docker compose --env-file .env up -d --build eventlake
```

Build the prebuilt deployment binary:

```text
scripts/build-prebuilt-binary.sh
```

Run the prebuilt binary image:

```text
EVENTLAKE_ENV_FILE=.env docker compose --env-file .env -f docker-compose.prebuilt.yml up -d --build eventlake
```

Run the China-optimized prebuilt image:

```text
EVENTLAKE_ENV_FILE=.env docker compose --env-file .env -f docker-compose.prebuilt.cn.yml up -d --build eventlake
```

Stop local Docker services:

```text
docker compose down
```

See `docs/DEPLOYMENT.md` for the deployment matrix, environment-file rules, and verification commands.

## 6. Expected Database Workflow

The implementation uses migrations for all PostgreSQL schema changes.

Expected workflow:

1. Add migration.
2. Run migration locally.
3. Run tests.
4. Commit migration with related code.

Migration rules:

- All EventLake-owned tables must use the `eventlake_` prefix.
- SQLx migration state must use `eventlake_sqlx_migrations`.
- Raw log tables must support partitioning.
- Decoded event tables must support partitioning.
- Index tables must be query-optimized from the start.
- Destructive migrations require explicit user approval.

## 7. Expected Runtime Workflow

After implementation, a normal local run should support:

1. Start PostgreSQL and EventLake.
2. Run database migrations.
3. Create or bootstrap admin credentials.
4. Add chain metadata if not seeded.
5. Add RPC endpoints.
6. Upload ABI.
7. Create contract subscription.
8. Observe sync progress.
9. Search events.

## 8. Expected Health Checks

The service exposes:

- Liveness check.
- Readiness check with PostgreSQL connectivity.

The routes are `/health/live` and `/health/ready`.

## 9. Expected OpenAPI Output

The implementation generates OpenAPI documentation from route and schema definitions.

OpenAPI must describe:

- Auth endpoints.
- Chain management endpoints.
- RPC pool endpoints.
- ABI endpoints.
- Subscription endpoints.
- Search endpoint.
- Explorer endpoints.
- Dashboard endpoints.

## 10. Testing Strategy

Test coverage should grow by risk:

- Unit tests for ABI parsing.
- Unit tests for Search DSL validation and SQL planning.
- Unit tests for RPC selection policy.
- Integration tests for subscription uniqueness.
- Integration tests for raw log persistence before decode.
- Integration tests for decoder and index builder.
- Integration tests for reorg repair.
- Real PostgreSQL E2E for API, migrations, ABI, subscription, collector, decoder, indexes, search, explorers, dashboard, auth, and reorg.

Large-chain behavior should be tested with bounded fixtures first, then load tests after core behavior is stable.

## 11. Local Usage Examples

These examples describe the current API.

Upload ABI:

```text
POST /api/abis
```

Create subscription:

```text
POST /api/subscriptions
```

Search:

```text
POST /api/search
```

View address:

```text
GET /api/explorer/address/{address}
```

View the operational dashboard:

```text
GET /api/dashboard
```

The API remains REST-based and is described at `GET /api/openapi.json`.

## 12. Current Verification Notes

Verified locally:

```text
cargo check
cargo build --release --locked
cargo test
cargo test --test e2e_real_database_tests -- --nocapture
cargo clippy --all-targets --all-features -- -D warnings
scripts/build-prebuilt-binary.sh
docker compose --env-file .env.example config
docker compose --env-file .env.example -f docker-compose.prebuilt.yml config
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config
```

Not verified in this environment:

- `docker build -f Dockerfile -t eventlake:local .`
- `docker build -f Dockerfile.prebuilt -t eventlake:prebuilt .`
- `docker build -f Dockerfile.prebuilt.cn -t eventlake:prebuilt-cn .`
- `docker compose up`
- End-to-end RPC collection against a real EVM chain

Reason:

- The current user cannot access `/var/run/docker.sock`.
- `.env.test` PostgreSQL migration and E2E execution are verified.
- E2E currently uses a deterministic local JSON-RPC HTTP fixture instead of a public chain RPC endpoint.

The migration is still compiled into the binary through `sqlx::migrate!("./migrations")`.
