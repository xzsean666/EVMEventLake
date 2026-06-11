# EventLake Next Session Handoff

Last updated: 2026-06-11

Repository: `/home/sean/git/EVMEventLake`

Branch: `main`

## 1. Current Progress

The project is in documentation stage.

Completed workflow steps:

- Step 1 - Architecture Design: completed.
- Step 2 - Documentation: completed.
- Step 3 - Context Handoff: this file.

Step 4 - Implementation has not been approved yet.

No Rust source code, migrations, Dockerfile, or docker-compose files have been created.

Recent commits:

- `65f5fe7 feat: add architecture design documentation`
- `3f8e8ab feat: add project documentation`

## 2. Canonical Documents

Read these files before making future changes:

- `Agent.md`
- `docs/ARCHITECTURE.md`
- `docs/SPEC.md`
- `docs/BUILD.md`
- `docs/EXTERNAL_DOCS.md`
- `docs/nextsession.md`

## 3. Architecture Summary

EventLake is an EVM event collection, indexing, and search platform.

V1 architecture:

- Rust monolith.
- PostgreSQL database.
- Docker deployment.
- Two services only: `postgres` and `eventlake`.
- No Kafka, ClickHouse, S3, Elasticsearch, or Redis in V1.

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
Search DSL queries indexed data
```

Primary module boundaries:

- `app` - process startup and application state.
- `api` - REST routes, response envelope, OpenAPI integration.
- `auth` - JWT, API key, roles.
- `configuration` - centralized typed runtime configuration.
- `database` - PostgreSQL pool, migrations, transactions.
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
- `docs/EXTERNAL_DOCS.md`
- `docs/nextsession.md`

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

## 5. Pending Tasks

Do not start these tasks until the user explicitly requests Step 4 implementation.

Recommended Step 4 sequence:

1. Create Rust project skeleton.
2. Add `Cargo.toml` with pinned dependencies.
3. Add `src/main.rs`, `src/app`, `src/configuration`, `src/database`, and `src/api` skeleton.
4. Add Dockerfile and docker-compose with only `postgres` and `eventlake`.
5. Add database migration framework.
6. Add health/readiness endpoints.
7. Add chain metadata and RPC endpoint registry.
8. Add ABI upload and event parser.
9. Add contract subscription creation with active uniqueness.
10. Add raw log storage schema.
11. Add historical collector.
12. Add realtime collector.
13. Add block checkpoints and reorg detection.
14. Add decoder.
15. Add address and event field indexes.
16. Add Search DSL validation and query planning.
17. Add explorer endpoints.
18. Add dashboard and job monitor endpoints.
19. Add auth with JWT and API keys.
20. Add integration tests and documentation updates.

## 6. Next Actions

If the user asks for more documentation:

- Refine `docs/SPEC.md` first.
- Then update `docs/ARCHITECTURE.md` if architecture changes.
- Then update this handoff.
- Commit after the documentation change.

If the user approves Step 4 implementation:

- State the current step before starting.
- Confirm that implementation code is now allowed.
- Start with the smallest bootable Rust service.
- Keep each module locally understandable.
- Commit after each major implementation phase.

## 7. Risks and Unknowns

Open decisions before implementation:

- Exact crate versions.
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

## 8. Rules for Future AI Sessions

Always follow `Agent.md`.

Do not write implementation code unless Step 4 is explicitly requested.

Do not introduce non-V1 infrastructure.

Do not scatter configuration or hidden global state.

Do not add "god" utility modules.

Do not create duplicate active subscriptions for the same `chain_id + contract_address`.

Do not generate SQL from unvalidated Search DSL fields.

Preserve raw logs before any decode or index action.

