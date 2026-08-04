# EventLake Architecture Design

Version: 1.1

Status: Implemented baseline

Language: Rust

Architecture: Monolith First

Database: PostgreSQL operational store + optional ClickHouse search store

Deployment: Docker

## 1. Architecture Goal

EventLake is an EVM event collection, indexing, and search platform.

The system is optimized for AI-assisted long-term development:

- Each module has one clear responsibility.
- Data flow is explicit and inspectable.
- The deployable unit is a single Rust service. PostgreSQL is always required;
  ClickHouse is optional and enabled by a feature plus a runtime flag.
- Raw blockchain logs are always preserved before decoding or indexing.
- PostgreSQL owns raw logs and all transactional/operational state. The search
  storage mode is selected at startup: PostgreSQL-only stores decoded events and
  indexes in PostgreSQL; ClickHouse mode stores those derived rows only in
  ClickHouse.
- Search behavior is treated as the primary product surface.

The system starts as a monolith to reduce distributed-system complexity. Internal module boundaries must still be strong enough to support future extraction into workers or services.

## 2. High-Level System Architecture

```text
Client / Admin UI / API Consumer
        |
        v
REST API + Auth + OpenAPI
        |
        v
Application Services
        |
        +--> ABI Management
        +--> Chain and RPC Management
        +--> Subscription and Job Management
        +--> Search Service
        +--> Explorer Services
        +--> Dashboard Service
        |
        v
Background Runtime
        |
        +--> RPC Health Checker
        +--> Historical Collector
        +--> Realtime Collector
        +--> Decoder
        +--> Partition Manager
        |
        v
PostgreSQL (raw-log and operational source of truth)
        |
        +--> Raw Logs
        +--> Contracts
        +--> Event Registry
        +--> Jobs and Checkpoints
        +--> Users and API Keys

        | raw log + decode queue are durable
        v
ClickHouse mode only: search projection
        |
        +--> decoded_events (ReplacingMergeTree)
        +--> address_index (ReplacingMergeTree)
        +--> event_field_index (ReplacingMergeTree)
```

## 3. Recommended Directory Structure

This structure is intentionally explicit. Directories are grouped by responsibility instead of technical novelty.

```text
.
├── Agent.md
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
├── docs/
│   ├── ARCHITECTURE.md
│   ├── SPEC.md
│   ├── BUILD.md
│   ├── USAGE.md
│   ├── EXTERNAL_DOCS.md
│   └── nextsession.md
├── migrations/
├── clickhouse/
│   └── schema.sql
├── src/
│   ├── main.rs
│   ├── app/
│   │   ├── mod.rs
│   │   ├── application_state.rs
│   │   └── startup.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── routes.rs
│   │   ├── response.rs
│   │   └── handlers/
│   ├── auth/
│   ├── configuration/
│   ├── database/
│   ├── clickhouse/       # feature-gated search projection
│   ├── chains/
│   ├── rpc_pool/
│   ├── abi_registry/
│   ├── subscriptions/
│   ├── collector/
│   ├── reorg/
│   ├── decoder/
│   ├── indexing/
│   ├── search/
│   ├── explorers/
│   ├── dashboard/
│   ├── background/
│   ├── telemetry/
│   └── shared/
└── tests/
```

## 4. Module Breakdown

### 4.1 app

Purpose:

- Own process startup and shared application state.
- Wire configuration, database pool, HTTP router, and background runtime.

Input:

- Environment variables.
- Configuration file if added later.
- Database connection settings.

Output:

- Running HTTP server.
- Running background workers.

Dependencies:

- `configuration`
- `database`
- `api`
- `background`
- `telemetry`

### 4.2 api

Purpose:

- Expose REST endpoints.
- Apply authentication and authorization.
- Normalize response format.
- Generate OpenAPI documentation.

Input:

- HTTP requests.
- JWT or API key credentials.

Output:

- Unified API responses:

```json
{
  "success": true,
  "data": {},
  "error": null,
  "meta": {}
}
```

Dependencies:

- `auth`
- application services from domain modules
- `search`
- `explorers`
- `dashboard`

### 4.3 auth

Purpose:

- Validate JWTs.
- Validate API keys.
- Enforce roles: `Admin`, `ReadOnly`.

Input:

- Authorization header.
- API key header.

Output:

- Authenticated principal with role.
- Authorization errors.

Dependencies:

- `database`
- `shared`

### 4.4 configuration

Purpose:

- Centralize all runtime configuration.
- Prevent scattered environment reads.

Input:

- Environment variables.
- Optional configuration file.

Output:

- Typed configuration structure.

Dependencies:

- none except standard parsing libraries.

### 4.5 database

Purpose:

- Own PostgreSQL connection setup.
- Provide transaction boundaries.
- Run migrations.
- Keep database access explicit.

Input:

- Database URL.
- Repository requests from modules.

Output:

- Query results.
- Transactions.

Dependencies:

- PostgreSQL driver.
- migration tooling.

### 4.5.1 clickhouse

Purpose:

- Maintain the optional derived-event search store used by the search path.
- Apply `clickhouse/schema.sql` at startup after a ClickHouse health check.
- In ClickHouse mode, write decoded events, address rows, and event-field rows
  after the PostgreSQL raw-log and queue transaction commits.
- Publish newer tombstone rows when PostgreSQL marks events as reorged.

Runtime behavior:

- The module is compiled only with the `clickhouse` Cargo feature.
- `EVENTLAKE_CLICKHOUSE_ENABLED=false` leaves the client absent and preserves the
  PostgreSQL-only path.
- A connection or write error is logged, leaves the decode queue retryable, and
  blocks the affected subscription until the ClickHouse write succeeds. Search
  returns a service error while its selected storage is unavailable; it does not
  silently return a partial PostgreSQL result.

Dependencies:

- `configuration::ClickHouseConfig`
- `database` for durable raw logs, queues, and reorg checkpoints
- `indexing` and `search` data contracts
- ClickHouse HTTP interface on port `8123` by default.

### 4.6 chains

Purpose:

- Maintain supported chain metadata.
- Allow dynamic chain registration.

Input:

- Chain ID.
- Chain name.
- Finality and reorg policy.
- Default block time.

Output:

- Chain metadata used by collectors, RPC pool, and search.

Dependencies:

- `database`

### 4.7 rpc_pool

Purpose:

- Manage RPC endpoints as independent resources.
- Track health, latency, weight, status, and last check time.
- Select a healthy RPC for a chain.

Input:

- Chain ID.
- RPC URL.
- RPC health check result.
- Runtime selection request.

Output:

- Healthy RPC endpoint.
- RPC status updates.
- Retry decision.

Dependencies:

- `chains`
- `database`
- EVM JSON-RPC client.

### 4.8 abi_registry

Purpose:

- Upload, update, delete, and version ABIs.
- Parse event definitions.
- Maintain event registry records.

Input:

- ABI JSON.
- Contract association request.

Output:

- ABI version.
- Event name.
- Event signature.
- Topic0.
- Indexed and non-indexed inputs.

Dependencies:

- ABI parser.
- `database`
- `shared`

### 4.9 subscriptions

Purpose:

- Represent the durable indexing intent for one chain and contract.
- Enforce the `chain_id + contract_address` active uniqueness rule.
- Track job state and checkpoints.

Input:

- Chain ID.
- Contract address.
- ABI ID.
- Start block.
- Realtime enabled flag.

Output:

- Existing or newly created subscription.
- Job status.
- Checkpoint updates.

Dependencies:

- `chains`
- `abi_registry`
- `database`

### 4.10 collector

Purpose:

- Fetch logs from EVM chains.
- Run historical sync and realtime sync.
- Store raw logs before any decoding.
- Queue decode work using internal database-backed state.

Input:

- Active subscription.
- RPC endpoint.
- Block range.

Output:

- Raw log records.
- Updated block checkpoints.
- Decode queue records.

Dependencies:

- `rpc_pool`
- `subscriptions`
- `reorg`
- `database`

### 4.11 reorg

Purpose:

- Detect and repair chain reorganizations.
- Maintain block number to block hash checkpoints.
- Mark removed logs and schedule affected ranges for repair.

Input:

- Chain ID.
- Block number.
- Observed block hash.
- Removed log flag from RPC if available.

Output:

- Reorg event.
- Invalidated raw logs and decoded events.
- Repair work items.

Dependencies:

- `collector`
- `database`

### 4.12 decoder

Purpose:

- Decode raw logs using ABI event definitions.
- Allow raw logs to remain stored when ABI is missing.
- Re-decode historical raw logs when ABI changes.

Input:

- Raw log.
- Event registry entry.
- ABI version.

Output:

- Decoded event.
- Decoding error record when decoding fails.
- PostgreSQL-only mode writes decoded rows and derived indexes in one PostgreSQL
  transaction. ClickHouse mode writes the raw log and queue in PostgreSQL, then
  writes decoded rows and derived indexes only to ClickHouse before completing the
  queue item.

Dependencies:

- `abi_registry`
- `database`
- optional `clickhouse`

### 4.13 indexing

Purpose:

- Build query-optimized derived indexes.
- Maintain address reverse index.
- Maintain event field index.
- Manage PostgreSQL partitions.

Input:

- Decoded event.
- Raw log metadata.
- Event schema.

Output:

- Address index records.
- Event field index records.
- Partition maintenance actions.

ClickHouse mode receives derived rows through the decoder after the raw log and
decode queue commit. It is not used for subscriptions, authentication, ABI data,
or checkpoints, and PostgreSQL does not also store those decoded/index rows.

Dependencies:

- `decoder`
- `database`
- optional `clickhouse`

### 4.14 search

Purpose:

- Provide one unified Search DSL.
- Compile validated search requests into efficient SQL.
- Prefer address index, event index, and partition pruning.

Input:

- Search DSL query.
- Pagination.
- Sorting.

Output:

- Search result rows.
- Pagination metadata.

PostgreSQL-only mode queries PostgreSQL decoded-event and derived-index tables.
ClickHouse mode queries `decoded_events FINAL` and its two ClickHouse indexes. The
selected store is never silently substituted: a ClickHouse outage makes search
unavailable rather than returning a partial, stale, or accidentally duplicated
PostgreSQL result.

Dependencies:

- `database`
- `shared`
- optional `clickhouse`

### 4.15 explorers

Purpose:

- Provide read models for address, contract, and event exploration.
- Reuse search and index tables instead of owning new collection logic.

Input:

- Address.
- Contract address.
- Event name or topic0.

Output:

- Recent events.
- Related contracts.
- Event statistics.
- Last activity.

Dependencies:

- `search`
- `database`

### 4.16 dashboard

Purpose:

- Provide operational metrics for the admin UI.
- Summarize sync lag, current block, throughput, active jobs, and RPC health.

Input:

- Job state.
- RPC health records.
- Collector metrics.

Output:

- Dashboard summary view.

Dependencies:

- `subscriptions`
- `rpc_pool`
- `database`

### 4.17 background

Purpose:

- Own background worker lifecycle inside the monolith.
- Schedule collector, decoder, RPC health, and partition maintenance tasks;
  reorg handling is invoked by the collector workflow.

Input:

- Application state.
- Worker configuration.

Output:

- Running async tasks.
- Graceful shutdown result.

Dependencies:

- `collector`
- `decoder`
- `indexing`
- `rpc_pool`
- `reorg`

### 4.18 telemetry

Purpose:

- Centralize logs, metrics, and tracing setup.
- Avoid ad-hoc logging behavior inside business modules.

Input:

- Runtime configuration.
- Structured events from modules.

Output:

- Logs.
- Metrics.
- Traces.

Dependencies:

- tracing and metrics libraries.

### 4.19 shared

Purpose:

- Hold narrow shared primitives only.
- Avoid becoming a general-purpose utility dump.

Allowed contents:

- Domain-neutral error type.
- Address, topic, block number value objects.
- Pagination and sorting structs.
- Clock abstraction if needed for tests.

Not allowed:

- Business logic.
- Database queries.
- RPC calls.
- Search compilation.

## 5. Core Data Flow

### 5.1 ABI Upload Flow

```text
User uploads ABI
        |
        v
api validates request
        |
        v
abi_registry stores ABI version
        |
        v
abi_registry parses events
        |
        v
event registry stores event name, signature, topic0, inputs
        |
        v
contract registry can associate ABI with contracts
```

### 5.2 Subscription Creation Flow

```text
User creates index job
        |
        v
api validates chain, contract, ABI, start block
        |
        v
subscriptions checks active unique key:
chain_id + contract_address
        |
        +--> if active subscription exists: return existing subscription
        |
        +--> if not: create subscription and checkpoint
        |
        v
background collector discovers runnable work
```

### 5.3 Historical Collection Flow

```text
subscription checkpoint
        |
        v
collector calculates block window
        |
        v
rpc_pool selects healthy RPC
        |
        v
collector fetches eth_getLogs
        |
        v
database stores raw logs first
        |
        v
reorg stores block checkpoints
        |
        v
decoder work is queued
        |
        v
subscription checkpoint advances
```

### 5.4 Realtime Collection Flow

```text
historical sync reaches near chain head
        |
        v
subscription enters realtime mode
        |
        v
collector polls or subscribes to new blocks
        |
        v
new logs follow same raw -> decode -> index path
```

### 5.5 Decode and Index Flow

```text
raw log
        |
        v
decoder finds event by topic0 and ABI version
        |
        +--> ABI found: decoded event and derived indexes are stored
        |
        +--> ABI missing: raw log remains retained and decode work stays retryable
        |
        v
indexing extracts address fields and searchable event fields
        |
        v
storage mode is selected at startup:
        |
        +--> PostgreSQL-only: decoded event, address index, and field index are updated
        |
        +--> ClickHouse: raw log and queue commit in PostgreSQL, then decoded event,
             address index, and field index are written only to ClickHouse
```

### 5.6 Search Flow

```text
client submits Search DSL
        |
        v
api authenticates request
        |
        v
search validates allowed fields and operators
        |
        v
search planner selects the configured storage path:
PostgreSQL decoded-event and derived indexes, or ClickHouse FINAL tables
        |
        v
database query returns paginated result
        |
        v
api returns unified response
```

## 6. Key Design Decisions

### 6.1 Monolith First

EventLake is one Rust service plus PostgreSQL, with an optional ClickHouse service
for analytical search.

Reason:

- Lower operational complexity.
- Easier AI-assisted navigation.
- Clearer transaction boundaries.
- Future modules can be extracted after behavior is proven.

### 6.2 PostgreSQL as the Transactional Source of Truth

PostgreSQL always stores raw logs, queues, jobs, checkpoints, metadata, users, and
API keys. The decoded event, address index, and event-field index are stored in one
place only: PostgreSQL in the default mode, or ClickHouse in ClickHouse mode.
ClickHouse does not own subscriptions, queues, auth, ABI metadata, or reorg state.

Reason:

- PostgreSQL provides transactional consistency and remains sufficient for the
  default two-service deployment.
- PostgreSQL can support partitioning, JSONB, B-tree indexes, BRIN indexes, and transactional consistency.
- ClickHouse is activated only when large, multi-filter analytical searches justify
  a third service, without duplicating the derived-event data on both disks.

### 6.3 Database-Backed Work Queues

Decoder, indexer, repair, and retry work should be represented by database rows in V1.

Reason:

- Keeps V1 deployable with two services.
- Allows crash recovery.
- Makes system state inspectable.

### 6.4 Raw Logs Are the Source of Truth

The system must store raw logs before decoding.

Reason:

- ABI may be missing or wrong.
- Decoding logic may improve.
- Historical re-decode must be possible without refetching from chain.
- ClickHouse search rows are reproducible from PostgreSQL raw logs plus ABI versions.
  A write outage is therefore retried from the durable decode queue instead of being
  acknowledged or skipped.

### 6.5 Search DSL Instead of Many Specialized Endpoints

Search must be exposed as one generic DSL plus explorer read models.

Reason:

- Reduces API sprawl.
- Keeps behavior consistent.
- Lets query optimization improve behind one interface.

### 6.6 Active Subscription Uniqueness

Only one active subscription can exist for the same `chain_id + contract_address`.

Reason:

- Prevents duplicate RPC usage.
- Prevents duplicate raw logs.
- Prevents inconsistent decoded/indexed records.

### 6.7 Partition by Block Number

PostgreSQL raw logs are partitioned by block range. In PostgreSQL-only mode the
decoded-event table is partitioned the same way; in ClickHouse mode ClickHouse
partitions its derived search rows by chain and a derived block-time bucket.

Reason:

- Most blockchain queries are naturally block-bounded.
- Recent activity and range queries can prune partitions.
- Retention and maintenance are easier.

### 6.8 Explicit Reorg Handling

Block hash checkpoints are required.

Reason:

- EVM logs can change when a chain reorganizes.
- Search results must not silently serve stale derived records.
- Repair behavior should be durable and observable.
- ClickHouse mode writes a newer row with `is_removed = true`; `FINAL` in the
  query path makes the tombstone visible before background merges complete. If that
  write fails, all affected subscriptions remain retryable and blocked.

### 6.9 Mature Rust Stack

Recommended V1 stack:

- HTTP: `axum`
- Async runtime: `tokio`
- Database: `sqlx`
- EVM primitives and RPC: `alloy`
- Serialization: `serde`
- OpenAPI: `utoipa`
- Telemetry: `tracing`

Reason:

- These libraries are widely used in Rust backend systems.
- They support explicit types and async workflows.
- They keep module boundaries readable.

## 7. Storage Architecture

Logical storage layers:

```text
PostgreSQL: raw_logs + decode queue + operational metadata (source truth)
        |
        +--> PostgreSQL-only: decoded_events + address_index + event_field_index
        |
        +--> ClickHouse mode: decoded_events + address_index + event_field_index
```

Core storage groups:

- Chain metadata.
- RPC endpoints.
- ABI versions.
- Event registry.
- Contract registry.
- Subscriptions and checkpoints.
- Block checkpoints.
- Raw logs.
- Decoded events, address index, and event-field index in the selected search store.
- Auth records.

Derived records in either search store must be reproducible from PostgreSQL raw logs
plus ABI versions.

## 8. Search Architecture

Search requests are compiled in three stages:

1. Validate DSL shape, operators, fields, pagination, and sorting.
2. Build an internal query plan that selects indexes and partition constraints.
3. Generate parameterized SQL.

Allowed operators:

- `eq`
- `neq`
- `gt`
- `gte`
- `lt`
- `lte`
- `contains`
- `starts_with`
- `ends_with`
- `in` and `not_in` are reserved in the request enum but are currently rejected
  by field executors.

Multiple filters are currently combined with `AND`; `OR` and `NOT` are not part
of the implemented request shape.

The SQL compiler must never pass arbitrary field names or operators directly from user input into SQL strings. Field names and sort keys must be mapped from a whitelist.

## 9. Operational Architecture

Background workers run inside the `eventlake` process:

- RPC health checker.
- Collector, including historical and realtime work.
- Decoder and derived-index writer.
- Partition manager.

Each worker must:

- Have a clear input table or query.
- Claim work explicitly.
- Write progress durably.
- Be restartable.
- Emit structured telemetry.

## 10. Future Evolution Boundaries

V1 intentionally avoids distributed infrastructure, but module boundaries should prepare for later phases:

- V2 Redis can accelerate locks, cache, and rate limiting.
- V3 multi-worker can extract background runtime into independent workers.
- V4 S3 archive can move cold raw logs to object storage.
- V5 can separate the ClickHouse writer or search executor if the monolith
  becomes a bottleneck.
- V6 distributed cluster can shard collection and query workloads.

The V1 architecture should not hard-code assumptions that block these phases.

## 11. Architecture Constraints

The following constraints are mandatory:

- PostgreSQL is the source of truth for raw logs and operational state; ClickHouse
  may only contain derived data.
- ClickHouse integration must remain feature-gated and runtime-optional.
- No Kafka, S3, Elasticsearch, Redis, or external queue is required for the
  default deployment.
- No duplicate active subscription for the same chain and contract.
- No decoded event without a stored raw log.
- No scattered environment variable access outside `configuration`.
- No hidden global mutable state.
- No generic utility module that accumulates business logic.
- No search SQL generated from unvalidated user fields.

## 12. AI-Oriented Maintainability Rules

Every future source file should answer these questions locally:

- What does this file own?
- What input does it accept?
- What output does it produce?
- Which modules does it depend on?
- Which side effects can it perform?

When a file cannot answer those questions clearly, the design should be split before more behavior is added.
