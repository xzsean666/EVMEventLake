# EventLake Architecture Design

Version: 1.0

Status: Draft

Language: Rust

Architecture: Monolith First

Database: PostgreSQL

Deployment: Docker

## 1. Architecture Goal

EventLake is an EVM event collection, indexing, and search platform.

The system is optimized for AI-assisted long-term development:

- Each module has one clear responsibility.
- Data flow is explicit and inspectable.
- The first version is a single deployable service with PostgreSQL.
- Raw blockchain logs are always preserved before decoding or indexing.
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
        +--> Reorg Detector
        +--> Decoder
        +--> Index Builder
        +--> Partition Manager
        |
        v
PostgreSQL
        |
        +--> Raw Logs
        +--> Decoded Events
        +--> Address Index
        +--> Event Field Index
        +--> Contracts
        +--> Event Registry
        +--> Jobs and Checkpoints
        +--> Users and API Keys
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
│   ├── EXTERNAL_DOCS.md
│   └── nextsession.md
├── migrations/
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

Dependencies:

- `abi_registry`
- `database`

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

Dependencies:

- `decoder`
- `database`

### 4.14 search

Purpose:

- Provide one unified Search DSL.
- Compile validated search requests into efficient SQL.
- Prefer address index, event index, and partition pruning.

Input:

- Search DSL query.
- Pagination.
- Sorting.
- Aggregation request.

Output:

- Search result rows.
- Aggregation result.
- Pagination metadata.

Dependencies:

- `database`
- `shared`

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
- Schedule collector, decoder, indexer, reorg, RPC health, and partition tasks.

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
        +--> ABI found: decoded event is stored
        |
        +--> ABI missing: raw log remains searchable by raw metadata
        |
        v
indexing extracts address fields and searchable event fields
        |
        v
address index and event field index are updated
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
search planner selects query path:
address index / event field index / raw metadata / partition range
        |
        v
database query returns paginated result
        |
        v
api returns unified response
```

## 6. Key Design Decisions

### 6.1 Monolith First

EventLake V1 is one Rust service plus PostgreSQL.

Reason:

- Lower operational complexity.
- Easier AI-assisted navigation.
- Clearer transaction boundaries.
- Future modules can be extracted after behavior is proven.

### 6.2 PostgreSQL as the Only V1 Storage Engine

Raw logs, decoded events, indexes, queues, jobs, checkpoints, users, and API keys all live in PostgreSQL.

Reason:

- The V1 deployment requirement allows only `postgres` and `eventlake`.
- PostgreSQL can support partitioning, JSONB, B-tree indexes, BRIN indexes, and transactional consistency.
- It avoids premature Kafka, Elasticsearch, S3, or ClickHouse complexity.

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

Raw logs and decoded events should be partitioned by chain and block range.

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
raw_logs
        |
        v
decoded_events
        |
        v
address_index + event_field_index
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
- Decoded events.
- Address index.
- Event field index.
- Auth records.

Derived records must be reproducible from raw logs plus ABI versions.

## 8. Search Architecture

Search requests should be compiled in three stages:

1. Validate DSL shape, operators, fields, pagination, sorting, and aggregation.
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
- `in`
- `not_in`

Allowed boolean logic:

- `AND`
- `OR`
- `NOT`

The SQL compiler must never pass arbitrary field names or operators directly from user input into SQL strings. Field names and sort keys must be mapped from a whitelist.

## 9. Operational Architecture

Background workers run inside the `eventlake` process:

- RPC health checker.
- Historical collector.
- Realtime collector.
- Reorg detector.
- Decoder.
- Index builder.
- Partition manager.
- Retry manager.

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
- V5 ClickHouse can add analytical search at larger scale.
- V6 distributed cluster can shard collection and query workloads.

The V1 architecture should not hard-code assumptions that block these phases.

## 11. Architecture Constraints

The following constraints are mandatory:

- No implementation code before explicit Step 4 approval.
- No Kafka, ClickHouse, S3, Elasticsearch, Redis, or external queue in V1.
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
