# EventLake Build and Usage Guide

Version: 1.0

Status: Draft

## 1. Current Stage

The repository is currently in architecture and documentation stage.

Implementation has not been approved yet.

The following files do not exist yet and must not be created until Step 4 is explicitly requested:

- `Cargo.toml`
- `src/`
- `migrations/`
- `Dockerfile`
- `docker-compose.yml`

This document defines the intended build and usage flow for the future implementation.

## 2. Expected Local Requirements

Future implementation should assume:

- Rust stable toolchain.
- Cargo.
- Docker.
- Docker Compose.
- PostgreSQL through Docker Compose for local development.

Recommended Rust stack:

- `tokio` for async runtime.
- `axum` for HTTP API.
- `sqlx` for PostgreSQL access and migrations.
- `alloy` for EVM primitives, ABI handling, and JSON-RPC integration.
- `serde` for serialization.
- `utoipa` for OpenAPI.
- `tracing` for structured telemetry.

## 3. Expected Environment Variables

All configuration must be centralized in the future `configuration` module.

Expected variables:

```text
EVENTLAKE_HTTP_HOST=0.0.0.0
EVENTLAKE_HTTP_PORT=8080
EVENTLAKE_DATABASE_URL=postgres://eventlake:eventlake@postgres:5432/eventlake
EVENTLAKE_JWT_SECRET=change-me
EVENTLAKE_LOG_LEVEL=info
EVENTLAKE_DEFAULT_PAGE_LIMIT=50
EVENTLAKE_MAX_PAGE_LIMIT=500
```

RPC endpoints should be stored and managed through the database, not hard-coded environment variables.

## 4. Expected Docker Services

V1 allows only:

- `postgres`
- `eventlake`

No Redis, Kafka, ClickHouse, S3, or Elasticsearch service is allowed in V1.

## 5. Expected Development Commands

These commands are the intended interface after Step 4 creates the implementation.

Run tests:

```text
cargo test
```

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

Stop local Docker services:

```text
docker compose down
```

## 6. Expected Database Workflow

The future implementation should use migrations for all database schema changes.

Expected workflow:

1. Add migration.
2. Run migration locally.
3. Run tests.
4. Commit migration with related code.

Migration rules:

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

The future service should expose:

- Liveness check.
- Readiness check.
- Database connectivity check.
- Background worker status.

The exact route names should be defined during API implementation.

## 9. Expected OpenAPI Output

The future implementation should generate OpenAPI documentation from route and schema definitions.

OpenAPI must describe:

- Auth endpoints.
- Chain management endpoints.
- RPC pool endpoints.
- ABI endpoints.
- Subscription endpoints.
- Search endpoint.
- Explorer endpoints.
- Dashboard endpoints.
- Job monitor endpoints.

## 10. Testing Strategy

Test coverage should grow by risk:

- Unit tests for ABI parsing.
- Unit tests for Search DSL validation and SQL planning.
- Unit tests for RPC selection policy.
- Integration tests for subscription uniqueness.
- Integration tests for raw log persistence before decode.
- Integration tests for decoder and index builder.
- Integration tests for reorg repair.

Large-chain behavior should be tested with bounded fixtures first, then load tests after core behavior is stable.

## 11. Local Usage Examples

These examples describe intended usage after implementation.

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

View job status:

```text
GET /api/jobs/{job_id}
```

Exact route names may be adjusted during Step 4, but the API must remain REST-based and OpenAPI-documented.

