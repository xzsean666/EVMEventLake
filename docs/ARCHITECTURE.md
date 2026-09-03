# EventLake Architecture Design

Version: 2.0

Status: Raw event lake implementation

Language: Rust

Architecture: Monolith first

Database: PostgreSQL operational store + optional ClickHouse raw-event store

Deployment: Docker

## 1. Goal

EventLake collects canonical EVM logs and makes their encoded data searchable. It
does not decode ABI events. ABI interpretation belongs to downstream consumers that
can choose their own ABI source, decoder version, and data model.

- PostgreSQL always owns operational state: chains, RPC endpoints, subscriptions,
  checkpoints, reorg observations, users, and API keys.
- `EVENTLAKE_CLICKHOUSE_ENABLED=true` makes ClickHouse the raw-log store. The
  collector does not write `eventlake_raw_logs` or `eventlake_decode_queue` in that
  mode.
- With ClickHouse disabled, raw logs remain in PostgreSQL for small deployments.
- New collection never populates decoded events or decoded-field indexes. Existing
  decoded tables and APIs are compatibility-only historical data.
- A subscription is either `contract` scoped or `all_events` scoped. `all_events`
  omits the JSON-RPC address filter and requires ClickHouse.

## 2. System Shape

```text
Client / API consumer
        |
        v
REST API + auth + OpenAPI
        |
        +--> subscription and checkpoint management (PostgreSQL)
        +--> raw-log search
        |
        v
Background runtime
        +--> RPC health checker
        +--> historical/realtime raw-log collector
        +--> PostgreSQL partition maintenance (PostgreSQL raw mode only)
        |
        +--> PostgreSQL operational state
        |
        +--> PostgreSQL raw_logs, when ClickHouse is disabled
        |
        +--> ClickHouse raw_logs, when ClickHouse is enabled
```

The service is a monolith so collection, reorg handling, checkpoint mutation, and
storage acknowledgement stay explicit. ClickHouse is never used for subscriptions,
authentication, RPC management, or checkpoints.

## 3. Storage Modes

| Data or responsibility | PostgreSQL raw mode | ClickHouse raw mode |
| --- | --- | --- |
| Chains, RPC, subscription, checkpoint, reorg, auth | PostgreSQL | PostgreSQL |
| Raw EVM logs | PostgreSQL | ClickHouse only |
| Raw-log search | PostgreSQL | ClickHouse `raw_logs FINAL` |
| ABI decode and derived indexes | Not written by new collection | Not written by new collection |

ClickHouse mode requires both the Cargo feature and the runtime flag:

```bash
cargo build --release --locked --features clickhouse
```

```env
EVENTLAKE_CLICKHOUSE_ENABLED=true
```

Starting a binary without the feature and setting the flag to true is a startup
error. This prevents an accidental fallback that would send a full-chain raw-log
workload to PostgreSQL.

## 4. Core Modules

### app and configuration

Own startup, typed environment configuration, shared state, and worker lifecycle.
Startup migrates PostgreSQL first, then connects ClickHouse and applies its
idempotent schema when selected.

### subscriptions

Stores durable collection intent and checkpoint state. `collection_scope=contract`
requires a normalized `contract_address`; `collection_scope=all_events` requires no
address and has one active subscription per chain. All-events subscriptions require
ClickHouse because their data volume is unbounded.

### collector

Fetches `eth_getLogs` over an adaptive block window. A single contract subscription
sends the address filter; multiple contract subscriptions at the same `(chain_id, current_block)`
height are dynamically bucketed and carpooled into a single `eth_getLogs` request with
`address: [...]` (up to `max_batch_addresses`), demuxing returned logs back to their
originating `subscription_id`s; an all-events subscription sends only `fromBlock` and
`toBlock`. It writes raw rows before advancing the subscription checkpoints.

In ClickHouse mode, an RPC response is batched into `raw_logs`. If that write fails,
the client is discarded and the checkpoint is unchanged, so the next tick retries
the identical block range. `ReplacingMergeTree` makes repeat writes idempotent at
`(chain_id, block_number, transaction_hash, log_index)`.

### reorg

PostgreSQL compares observed block hashes and atomically rewinds affected
subscriptions. In PostgreSQL raw mode it marks local rows removed. In ClickHouse raw
mode it writes newer `is_removed=true` versions into `raw_logs`; all reads use
`FINAL`, so stale fork logs disappear before background merges complete.

### clickhouse

Creates and uses `raw_logs`, a `ReplacingMergeTree(stored_at)` table ordered by
`(chain_id, block_number, transaction_hash, log_index)`. The table stores positional
columns `topic0` through `topic3`, the complete JSON topic array, data, transaction
metadata, and tombstone state. Bloom-filter skipping indexes exist for each topic
position.

Historical decoded-event tables remain in the schema solely so a pre-upgrade
deployment can read old derived data. Collector and background runtime do not write
or decode into them.

### search

`POST /api/raw-logs/search` is the primary raw log search API. It uses the raw encoded
values directly and supports `chain_id`, `block_number`, `contract_address`,
`transaction_hash`, and `topic0` through `topic3`. A positive `chain_id eq` filter
is mandatory to retain predictable ClickHouse partition pruning. Filters combine
with `AND`; topic positions require exact, normalized 32-byte values.

`POST /api/search` is the previous decoded-event API. It is retained for old data
but receives no new collector output in raw-event-lake mode.

`POST /api/subscriptions/batch` creates multiple contract-scoped raw subscriptions
without requiring ABI identifiers. Addresses are normalized and de-duplicated before
creation, and an existing active subscription is returned on retry.

### block_transaction

Manages full-chain canonical EVM block and transaction data collection and query APIs:
- Background worker `block_transaction::worker` operates independently from raw-log
  workers and uses `eth_getBlockByNumber(blockNumber, true)`.
- ClickHouse `blocks` table (ReplacingMergeTree on `stored_at`, partitioned by
  `toYYYYMM(toDateTime(timestamp))`, primary key `(chain_id, block_number)`).
- ClickHouse `transactions` table (ReplacingMergeTree on `stored_at`, partitioned by
  `chain_id`, primary key `(chain_id, block_number, transaction_index, tx_hash)`).
- PostgreSQL table `eventlake_block_transaction_sync_state` tracks chain sync bounds,
  checkpoints (`next_block`, `safe_head`, `latest_seen_block`), and status.
- Reorg recovery invalidates blocks and transactions `>= from_block` via tombstone rows
  (`is_canonical = false`), while all queries enforce `FINAL` and `is_canonical = true`.
- MVP read APIs provide `/api/chains/{chain_id}/blocks/{block_ref}`,
  `/api/chains/{chain_id}/blocks/{block_ref}/transactions`,
  `/api/chains/{chain_id}/transactions/{tx_hash}`, and keyset-paginated
  `/api/chains/{chain_id}/addresses/{address}/transactions`.


## 5. Main Flows

### 5.1 Create an all-events subscription

```text
POST /api/subscriptions { collection_scope: all_events, chain_id, start_block }
        |
        v
validate ClickHouse mode and chain metadata
        |
        v
store PostgreSQL subscription/checkpoint with a NULL contract address
        |
        v
collector calls eth_getLogs with no address filter
```

### 5.2 Collect and store

```text
subscription checkpoint -> RPC eth_getLogs -> observe block hashes
        |
        +--> reorg: rewind PostgreSQL checkpoint and tombstone raw storage
        |
        +--> PostgreSQL raw mode: upsert eventlake_raw_logs
        |
        +--> ClickHouse raw mode: batch insert raw_logs
                                         |
                              insert succeeds before checkpoint advances
```

### 5.3 Search raw logs

```text
POST /api/raw-logs/search
        |
        v
validate whitelisted filters and mandatory chain_id
        |
        v
query selected raw store, excluding tombstones
        |
        v
return encoded topics and data with pagination metadata
```

### 5.4 Dynamic Multi-Address Carpool Collection

```text
runnable subscriptions
        |
        v
bucket by (chain_id, current_block, status) & chunk by max_batch_addresses
        |
        v
single eth_getLogs(address: [A, B, C...], from_block, to_block)
        |
        v
observe block hashes for reorg detection
        |
        v
demux logs by log.address -> assign subscription_id
        |
        v
batch write raw logs (PostgreSQL or ClickHouse)
        |
        v
advance checkpoints for all batched subscriptions
```

## 6. Design Constraints

- No new ABI decoding, decode queues, decoded events, or derived indexes are written
  by the background runtime.
- ClickHouse raw mode must not write raw logs to PostgreSQL.
- A collector checkpoint advances only after the selected raw storage has accepted
  the complete RPC response.
- Reorged raw logs must be hidden immediately by `removed=false` or
  `is_removed=false` query predicates.
- No full-chain subscription without ClickHouse.
- Search SQL is generated only from whitelisted fields, sort keys, and operators.
- PostgreSQL remains required even in ClickHouse raw mode because it owns durable
  operational state and reorg/checkpoint coordination.
