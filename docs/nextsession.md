# EventLake Next Session Handoff

Last updated: 2026-08-04

Repository: `/home/sean/git/EVMEventLake`

Branch: `main`

## 1. Current Progress

The project is in Step 4 implementation stage.

Completed workflow steps:

- Step 1 - Architecture Design: completed.
- Step 2 - Documentation: completed.
- Step 3 - Context Handoff: completed.
- Step 4 - Implementation: first monolith implementation completed, further hardening pending.

Rust source code, migrations, Dockerfile, and docker-compose files have been created.

Use this command to inspect recent commits:

```text
git log --oneline --decorate -5
```

## 2. Canonical Documents

Read these files before making future changes:

- `Agent.md`
- `docs/ARCHITECTURE.md`
- `docs/SPEC.md`
- `docs/BUILD.md`
- `docs/USAGE.md`
- `docs/EXTERNAL_DOCS.md`
- `docs/nextsession.md`

## 3. Architecture Summary

EventLake is an EVM event collection, indexing, and search platform.

Current architecture:

- Rust monolith.
- PostgreSQL transactional source of truth.
- Optional, feature-gated ClickHouse derived-event search store.
- Docker deployment.
- Default services: `postgres` and `eventlake`; ClickHouse Compose variants add
  `clickhouse`.
- No Kafka, S3, Elasticsearch, Redis, or external queue is required.

Core data flow:

```text
ABI upload
        |
        v
Event registry
        |
        v
Contract subscription
        |
        v
Collector fetches logs
        |
        v
Raw logs are stored
        |
        v
Decoder creates decoded events
        |
        v
Indexer creates address and field indexes
        |
        v
In ClickHouse mode, durable raw-log and queue rows are followed by ClickHouse-only
decoded-event/index writes
        |
        v
Search DSL queries the selected PostgreSQL or ClickHouse store; ClickHouse failures
leave the queue retryable and the affected subscription blocked
```

Primary module boundaries:

- `app` - process startup and application state.
- `api` - REST routes, response envelope, OpenAPI integration.
- `auth` - JWT, API key, roles.
- `configuration` - centralized typed runtime configuration.
- `database` - PostgreSQL pool, migrations, transactions.
- `clickhouse` - optional derived-event search store, schema initialization, search, and reorg tombstones.
- `chains` - chain metadata and dynamic chain registration.
- `rpc_pool` - independent RPC resources, health, selection, retries.
- `abi_registry` - ABI versions, event parsing, event registry.
- `subscriptions` - active contract indexing intent and checkpoints.
- `collector` - historical and realtime log collection.
- `reorg` - block hash checkpoints and reorg repair.
- `decoder` - ABI-based raw log decoding.
- `indexing` - address index, event field index, partition management.
- `search` - Search DSL validation, planning, SQL compilation.
- `explorers` - address, contract, and event read models.
- `dashboard` - operational summaries.
- `background` - worker lifecycle inside the monolith.
- `telemetry` - logs, metrics, tracing.
- `shared` - narrow domain-neutral primitives only.

## 4. Completed Parts

Created:

- `Agent.md`
- `docs/ARCHITECTURE.md`
- `docs/SPEC.md`
- `docs/BUILD.md`
- `docs/USAGE.md`
- `docs/EXTERNAL_DOCS.md`
- `docs/nextsession.md`
- `Cargo.toml`
- `Dockerfile`
- `Dockerfile.clickhouse`
- `docker-compose.yml`
- `docker-compose.clickhouse.yml`
- `clickhouse/schema.sql`
- `migrations/202606110001_initial_schema.sql`
- `src/`
- `tests/`

Documented:

- AI step workflow.
- Architecture design.
- Module responsibilities.
- Data flow.
- Key design decisions.
- Product specification.
- Build and usage expectations.
- External official docs links.
- Future implementation gate.
- Rust monolith service skeleton.
- Configuration, telemetry, database migration loader.
- REST API routes and unified response envelope.
- API key/JWT authentication support.
- Chain metadata endpoints.
- RPC endpoint registry and health checks.
- ABI upload and Event Registry parsing.
- Contract subscription creation with active uniqueness.
- Background collector, decoder, indexer, and partition manager workers.
- Raw log, decoded event, address index, and event field index storage schema.
- Database namespace isolation with the `eventlake_` table prefix and `eventlake_sqlx_migrations`.
- Search DSL endpoint.
- Address, contract, and event explorer endpoints.
- Dashboard summary endpoint.
- Unit/integration tests for ABI parsing, Search DSL validation, and address/topic validation.
- Real PostgreSQL E2E coverage through `tests/e2e_real_database_tests.rs`.
- ClickHouse feature, ClickHouse-only derived writes, durable retry states, and
  integration coverage through `tests/clickhouse_integration_tests.rs`.
- End-user quick-start and API workflow in `docs/USAGE.md`.

## 5. Pending Tasks

Recommended next hardening sequence:

1. Dynamic Multi-Address Carpool Collection implementation (`docs/dynamic-address-aggregation-upgrade/01-upgrade-design.md`):
   - Refactor `eth_get_logs` in `src/rpc_pool/evm_rpc_client.rs` to support multi-address array filter.
   - Introduce `max_batch_addresses` in `src/configuration/mod.rs`.
   - Update `src/collector/worker.rs` to bucket subscriptions by `(chain_id, current_block, status)` and perform batch collection with log demuxing.
   - Add unit and integration tests.
2. Exercise end-to-end flow with a real public or private EVM RPC endpoint:
   add RPC, upload ERC-20 ABI, create subscription, collect logs, decode, search.
3. Expand Search DSL support for `in`, `not_in`, and more field comparison types.
4. Add explicit Job Monitor routes separate from subscription routes if the UI needs different read models.
5. Improve OpenAPI annotations with concrete path schemas for all handlers.
6. Add JWT issuance or external identity provider integration if needed.
7. Add per-chain/provider RPC range-limit metadata.
8. Add stronger reorg repair tests.
9. Add load tests for recent search and address index performance.

## 6. Next Actions

If the user asks for more documentation:

- Refine `docs/SPEC.md` first.
- Then update `docs/ARCHITECTURE.md` if architecture changes.
- Then update this handoff.
- Commit after the documentation change.

If continuing implementation:

- Keep module boundaries from `docs/ARCHITECTURE.md`.
- Run `cargo fmt`, `cargo test`, and strict clippy before committing.
- Prefer database-backed integration tests once PostgreSQL access is available.

## 7. Risks and Unknowns

Open decisions:

- Exact Search DSL JSON shape.
- Partition size per chain and block range.
- Default confirmation depth for Ethereum, Base, Arbitrum, Optimism, Polygon, and BSC.
- RPC provider rate-limit handling model.
- Whether realtime collection starts with polling only or supports WebSocket in V1.
- ABI scope: global ABI registry first or contract-scoped ABI association first.
- Initial admin bootstrap strategy.
- How strict delete behavior should be for subscriptions and ABI versions.
- Load-test target for "millisecond-level recent search."

Known external concern:

- BNB Chain public RPC documentation notes `eth_getLogs` limitations on listed public mainnet endpoints. BSC support should rely on user-managed RPC endpoints and capability health checks.

Verification status:

- `cargo check`: passed.
- `cargo test`: passed.
- `cargo test --test e2e_real_database_tests -- --nocapture`: passed against `.env.test` PostgreSQL.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Docker runtime verification: blocked by local Docker socket permissions.
- Public-chain RPC E2E: not yet run; current E2E uses a deterministic local JSON-RPC HTTP fixture.

## 8. Rules for Future AI Sessions

Always follow `Agent.md`.

Do not introduce non-V1 infrastructure.

Do not scatter configuration or hidden global state.

Do not add "god" utility modules.

Do not create duplicate active subscriptions for the same `chain_id + contract_address`.

Do not generate SQL from unvalidated Search DSL fields.

Preserve raw logs before any decode or index action.

All EventLake-owned database tables must keep the `eventlake_` prefix. Tests must not drop shared schemas or unprefixed tables.
