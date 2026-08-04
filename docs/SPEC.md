# EventLake System Specification

Version: 1.1

Status: Current implementation

Language: Rust

Architecture: Monolith First

Database: PostgreSQL operational store + optional ClickHouse search store

Deployment: Docker

## 1. Product Vision

EventLake is an EVM event collection, indexing, and search platform.

The system continuously collects blockchain event logs, preserves raw data, decodes events from ABI definitions, creates high-performance search indexes, and exposes a unified API for search and exploration.

Target users:

- DeFi developers.
- Data analysis teams.
- Quantitative trading teams.
- Web3 SaaS products.
- Blockchain infrastructure teams.

## 2. Success Criteria

V1 is successful when the system can:

- Upload and version ABIs.
- Create a contract subscription for a chain and contract.
- Continuously sync historical and realtime events.
- Store raw logs before derived data.
- Decode events from known ABIs.
- Build address and field indexes automatically.
- Search recent events with millisecond-level latency under practical recent-data workloads.
- Support growth toward tens of millions and hundreds of millions of event records without changing the V1 architecture.

## 3. Non-Goals for V1

The default deployment must not require:

- Kafka.
- S3 archive.
- Elasticsearch.
- Redis.
- Multi-service worker deployment.
- Distributed cluster management.

ClickHouse is an optional feature-gated search store for large event volumes. It is
not required for the PostgreSQL-only default deployment. PostgreSQL always retains
raw logs and operational state; decoded events and indexes are stored in exactly one
selected search store.

## 4. Supported Chains

V1 must support:

- Ethereum.
- Base.
- Arbitrum.
- Optimism.
- Polygon.
- BSC.

The system must allow dynamic chain addition through chain metadata records.

Each chain should define:

- Chain ID.
- Chain name.
- Native token symbol.
- RPC finality behavior.
- Safe confirmation depth.
- Default maximum log block window.
- Whether public RPC endpoints have known `eth_getLogs` limitations.

## 5. Core Principles

### 5.1 Continuous Collection

All collection tasks run continuously by default.

When historical sync catches up to the latest usable block, the subscription enters realtime mode instead of completing.

### 5.2 Index Once

Only one active subscription may exist for the same `chain_id + contract_address`.

Duplicate create requests must return the existing active subscription instead of creating duplicate collection work.

### 5.3 Raw Data First

Raw logs are permanent source records.

Decoded events and indexes are derived records and must be rebuildable from:

- Raw logs.
- ABI versions.
- Event registry metadata.

### 5.4 Search First

Collection exists to serve search.

API design, storage layout, indexes, partitioning, and background processing must prioritize fast and predictable query behavior.

## 6. Main Workflow

```text
User uploads ABI
        |
        v
System parses ABI
        |
        v
User creates index job
        |
        v
System creates or returns contract subscription
        |
        v
Collector fetches logs
        |
        v
Raw logs are stored
        |
        v
Events are decoded when ABI is available
        |
        v
Indexes are built
        |
        v
User searches data
```

## 7. Domain Concepts

### 7.1 Chain

A supported EVM-compatible network.

Required fields:

- Chain ID.
- Name.
- Status.
- Safe confirmation depth.
- Default collection window.
- Created time.
- Updated time.

### 7.2 RPC Endpoint

An RPC endpoint is managed independently from jobs.

Required fields:

- Chain ID.
- URL.
- Status.
- Weight.
- Latency.
- Last check time.
- Failure count.
- Last error.

### 7.3 ABI Version

An uploaded ABI snapshot.

Required fields:

- ABI ID.
- Version.
- Original JSON.
- Parsed events.
- Created by.
- Created time.
- Status.

### 7.4 Event Registry Entry

A parsed event definition.

Required fields:

- Event name.
- Canonical signature.
- Topic0.
- Inputs.
- Indexed inputs.
- ABI version.

### 7.5 Contract Subscription

The durable intent to index one contract on one chain.

Required fields:

- Chain ID.
- Contract address.
- ABI ID.
- Start block.
- Current checkpoint.
- Historical sync status.
- Realtime sync status.
- Active flag.
- Error state.

### 7.6 Raw Log

The original log returned by the RPC layer.

Required fields:

- Chain ID.
- Contract address.
- Block number.
- Block hash.
- Transaction hash.
- Transaction index.
- Log index.
- Topic list.
- Data payload.
- Removed flag.
- Ingested time.

### 7.7 Decoded Event

A derived record created from a raw log and ABI event definition.

Required fields:

- Raw log reference.
- ABI version.
- Event name.
- Topic0.
- Decoded indexed fields.
- Decoded non-indexed fields.
- Decode status.
- Decode error if any.

### 7.8 Address Index

A reverse index for addresses discovered in decoded event fields.

Required fields:

- Chain ID.
- Address.
- Contract address.
- Event name.
- Field name.
- Raw log reference.
- Block number.
- Transaction hash.

### 7.9 Event Field Index

A field-level index for decoded event fields.

Required fields:

- Chain ID.
- Contract address.
- Event name.
- Field name.
- Field type.
- Normalized field value.
- Raw log reference.
- Block number.

## 8. ABI Module Specification

Capabilities:

- Upload ABI.
- Delete ABI.
- Update ABI.
- Version ABI.
- Parse events.
- Register event definitions.
- Associate ABI versions with contracts.

Parsing requirements:

- Event name.
- Canonical signature.
- Topic0.
- Inputs.
- Indexed inputs.
- Non-indexed inputs.
- Solidity types.

Rules:

- ABI uploads must not delete existing raw logs.
- ABI updates must create a new version.
- Re-decoding historical logs must be possible after ABI changes.
- Invalid ABI input must produce a clear validation error.

## 9. RPC Pool Module Specification

Capabilities:

- Add RPC endpoint.
- Delete RPC endpoint.
- Disable RPC endpoint.
- Enable RPC endpoint.
- Health check endpoints.
- Select healthy endpoint by chain.
- Retry failed requests.
- Recover disabled endpoint when checks pass.

Selection rules:

- Prefer healthy endpoints.
- Respect endpoint weight.
- Track latency.
- Avoid repeatedly selecting failing endpoints.
- Expose endpoint status to the dashboard.

Failure behavior:

- If one RPC fails, switch to another available RPC on the same chain.
- If all RPCs fail, mark subscription work as retryable instead of losing progress.

## 10. Subscription and Job Specification

Create request must include:

- Chain.
- Contract address.
- ABI.
- Start block.
- Realtime enabled flag.

Rules:

- `chain_id + contract_address` must have only one active subscription.
- Duplicate active create requests return the existing subscription.
- A subscription may be paused, resumed, or deleted.
- Delete should stop future work but must not delete raw logs by default.

Job states:

- `pending`
- `historical_syncing`
- `realtime_syncing`
- `paused`
- `error`
- `deleted`

## 11. Collector Specification

Capabilities:

- Historical sync.
- Realtime sync.
- Automatic retry.
- Automatic recovery.
- Dynamic block window sizing.
- Concurrent sync across independent subscriptions.

Historical sync rules:

- Fetch logs by bounded block ranges.
- Store raw logs before queueing decode.
- Advance checkpoints only after raw logs and block checkpoints are durable.
- Shrink block window after provider errors or timeouts.
- Grow block window cautiously after stable success.

Realtime sync rules:

- Continue running after historical sync catches up.
- Use a chain-specific confirmation depth.
- Avoid indexing blocks that are too new for the chain's reorg policy.

## 12. Reorg Specification

Capabilities:

- Store block checkpoints.
- Detect block hash changes.
- Detect removed logs when RPC returns removed flags.
- Mark affected derived records invalid.
- Schedule repair for affected block ranges.

Rules:

- Reorg repair must not delete raw history blindly.
- Derived decoded events and indexes must reflect the canonical chain state.
- Reorg events must be observable in job monitor and logs.

## 13. Decoder Specification

Capabilities:

- Decode indexed parameters from topics.
- Decode non-indexed parameters from data payload.
- Match event definitions by topic0.
- Store decode failures.
- Re-decode historical raw logs when ABI becomes available or changes.

Rules:

- Raw logs without ABI remain stored.
- Decoded events must reference a raw log.
- Decode output must identify ABI version.
- Decode must be deterministic for the same raw log and ABI version.

## 14. Storage Specification

Logical storage layers:

```text
Raw Logs
        |
        v
Decoded Events
        |
        v
Indexes
```

Partitioning:

- Raw logs and decoded events must be partitioned by block number.
- Partition management must be automatic.
- Users must not need to manually create partitions.

Indexing:

- Address fields must be indexed outside JSON payloads.
- Event fields must be indexed outside JSON payloads.
- Recent block queries must use partition pruning.
- Range queries must avoid full table scans where practical.

## 15. Contract Registry Specification

The system must maintain a contract catalog.

Required fields:

- Chain ID.
- Contract address.
- ABI ID.
- Event count.
- First seen block.
- Last seen block.
- First seen time.
- Last seen time.

Queries supported:

- Which events does this contract emit?
- How active is this contract?
- Which ABI version is currently associated?

## 16. Event Registry Specification

The system must maintain an event catalog.

Required fields:

- Event name.
- Signature.
- Topic0.
- Input schema.
- Contracts using it.
- Total count.

Queries supported:

- Which contracts support a given event?
- What topic0 maps to this event?
- What fields are searchable for this event?

## 17. Search Engine Specification

The system must expose one unified search interface.

Supported search categories:

- Address search.
- Contract search.
- Event search.
- Topic search.
- Transaction search.
- Block search.
- Time search.
- Field search.

The system must avoid many narrow search endpoints.

### 17.1 Search DSL

Supported comparison operators:

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

Supported logical operators:

- `AND`
- `OR`
- `NOT`

Supported result controls:

- Pagination.
- Sorting.
- Aggregation.

Security rules:

- All field names must be whitelisted.
- Operators must be whitelisted.
- Sorting keys must be whitelisted.
- Query compilation must produce parameterized SQL.
- User-provided field names must never be directly interpolated into SQL.

## 18. Explorer Specification

### 18.1 Address Explorer

Input:

- Address.
- Optional chain filter.

Output:

- Recent events.
- Related contracts.
- Event statistics.
- Last activity.

### 18.2 Contract Explorer

Input:

- Contract address.
- Optional chain filter.

Output:

- Event types.
- Event count.
- Recent activity.
- ABI association.

### 18.3 Event Explorer

Input:

- Event name or topic0.
- Optional chain filter.

Output:

- Signature.
- Topic0.
- Contracts using it.
- Total count.
- Searchable fields.

## 19. Dashboard and Job Monitor Specification

Dashboard must show:

- Current block per chain.
- Sync lag.
- Events per second.
- Logs per second.
- Active jobs.
- RPC health.
- Recent errors.

Job monitor must show:

- Sync progress.
- Historical progress.
- Realtime status.
- Current block.
- Error state.
- Pause, resume, and delete actions.

## 20. Authentication Specification

Supported authentication:

- JWT.
- API key.

Roles:

- `Admin`
- `ReadOnly`

Rules:

- Admin can mutate system state.
- ReadOnly can search and view explorer/dashboard data.
- API keys must be revocable.
- Secrets must not be logged.

## 21. API Design Specification

API style:

- REST.
- OpenAPI generated from implementation.
- Unified response envelope.

Response envelope:

```json
{
  "success": true,
  "data": {},
  "error": null,
  "meta": {
    "page": 1,
    "limit": 50
  }
}
```

List behavior:

- `page`
- `limit`
- `sort`
- `filter`

Primary API groups:

- Auth.
- Chains.
- RPC endpoints.
- ABIs.
- Subscriptions.
- Search.
- Address explorer.
- Contract explorer.
- Event explorer.
- Dashboard.

## 22. Deployment Specification

The default deployment has exactly two services:

- `postgres`
- `eventlake`

The ClickHouse-enabled deployment adds one optional `clickhouse` service. It stores
the derived search rows instead of PostgreSQL; PostgreSQL remains authoritative for
raw logs and operational state.

Required runtime concerns:

- Database migrations.
- Health checks.
- Graceful shutdown.
- Structured logs.
- Environment-based configuration.

## 23. Roadmap

V2:

- Redis for cache, locks, or rate limiting.

V3:

- Multi-worker deployment.

V4:

- S3 archive for cold raw logs.

V5:

- Separate the ClickHouse writer or search executor if the monolith becomes
  a bottleneck.

V6:

- Distributed cluster.

Roadmap items must not be required for V1.

## 24. Open Decisions

These decisions remain for hardening and future product work:

- Exact Rust crate versions.
- Exact partition size by block range per chain.
- Default confirmation depth per supported chain.
- Whether realtime sync uses polling first or WebSocket first.
- Whether ABI upload is global first or contract-scoped first.
- Search DSL JSON shape.
- Initial admin user bootstrap strategy.
