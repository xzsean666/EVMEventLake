# ClickHouse Raw Event Lake Upgrade

Version: 2.0

Status: Current implementation

Date: 2026-08-05

## 1. Goal

ClickHouse is the raw EVM-log store for large deployments. It is not a decoded-event
projection. With ClickHouse enabled, the collector writes encoded logs to ClickHouse
only; PostgreSQL retains only operational state.

| Data or responsibility | PostgreSQL-only mode | ClickHouse raw mode |
| --- | --- | --- |
| Chain, RPC, subscription, checkpoint, auth | PostgreSQL | PostgreSQL |
| Raw EVM log | PostgreSQL | ClickHouse only |
| Raw-log search | PostgreSQL | ClickHouse |
| ABI decoding and derived indexes | No new writes | No new writes |

This service no longer decodes. Consumers that need decoded events should read raw
data and apply their own ABI registry and decoding policy.

## 2. Enable the Mode

Build the feature-enabled binary and set the runtime flag:

```bash
cargo build --release --locked --features clickhouse
docker compose --env-file .env -f docker-compose.clickhouse.yml up -d --build
```

```env
EVENTLAKE_CLICKHOUSE_ENABLED=true
```

The flag is rejected if the binary was not compiled with `--features clickhouse`.
There is deliberately no PostgreSQL raw-log fallback in this mode.

## 3. Write and Retry Semantics

```text
RPC eth_getLogs response
        |
        v
observe PostgreSQL block checkpoints / detect reorg
        |
        v
batch INSERT ClickHouse raw_logs
        |
        +--> success: advance PostgreSQL subscription checkpoint
        |
        +--> failure: do not advance checkpoint; reconnect and retry range next tick
```

`raw_logs` uses `ReplacingMergeTree(stored_at)` ordered by `(chain_id,
block_number, transaction_hash, log_index)`. A retry can repeat an RPC response
without producing another logical log. The collector clears a failed client so the
next attempt health-checks ClickHouse and reapplies idempotent DDL.

A failed raw write transitions the subscription through its normal `error` retry
path. It is safe because the checkpoint remains before the failed interval. A
ClickHouse outage therefore pauses progress rather than losing logs.

## 4. Reorg Semantics

PostgreSQL is still responsible for block-hash observation and subscription rewind.
When a hash changes, ClickHouse mode inserts a newer `is_removed=true` version for
every `raw_logs` row at or above the reorg block. Canonical re-collection writes new
`is_removed=false` rows. All raw-log queries use `FINAL` and filter out tombstones,
so stale forks are not visible before ClickHouse background merges run.

If the tombstone write fails, subscriptions enter `clickhouse_reorg_retrying` and do
not collect the canonical fork until the tombstone succeeds.

## 5. Full-Chain Collection

Create an all-events subscription without a contract address:

```bash
curl -sS -X POST http://127.0.0.1:8080/api/subscriptions \
  -H 'content-type: application/json' \
  -d '{
    "chain_id": 1,
    "collection_scope": "all_events",
    "start_block": 22000000,
    "realtime_enabled": true,
    "min_block_window": 1,
    "max_block_window": 100
  }'
```

`all_events` requires ClickHouse and calls `eth_getLogs` with no `address` filter.
RPC providers have different log-result limits, so begin with a conservative window.
The collector shrinks the window for recognised range, response-size, and timeout
errors. Contract subscriptions remain available with
`collection_scope: "contract"` (or omitted, for compatibility).

For multiple contract-scoped raw subscriptions without ABI decoding, use the subscription
batch endpoint with contract_addresses and start_block. Input addresses are normalized and
de-duplicated, and an existing active subscription is returned instead of creating duplicate work.

## 6. Raw-Log Search

Use `POST /api/raw-logs/search`. A positive `chain_id eq` filter is mandatory. The
supported fields are `chain_id`, `block_number`, `contract_address`,
`transaction_hash`, and positional `topic0`, `topic1`, `topic2`, `topic3`.

```bash
curl -sS -X POST http://127.0.0.1:8080/api/raw-logs/search \
  -H 'content-type: application/json' \
  -d '{
    "page": 1,
    "limit": 100,
    "filters": [
      {"field":"chain_id","operator":"eq","value":1},
      {"field":"block_number","operator":"gte","value":22000000},
      {"field":"block_number","operator":"lte","value":22000100},
      {"field":"topic0","operator":"eq","value":"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"}
    ],
    "sort":{"field":"block_number","direction":"asc"}
  }'
```

Topics must be exact normalized 32-byte hex strings. The response includes the
complete `topics` array, `data`, block/transaction metadata, and ingestion time.

## 7. Tables and Operations

`clickhouse/schema.sql` creates `raw_logs` alongside legacy decoded-event tables.
New collection uses only `raw_logs`; legacy tables are retained so an existing API
consumer can still read previously decoded history.

`raw_logs` includes bloom-filter skipping indexes for `topic0` to `topic3`, a block
ordered primary key, and a persisted tombstone version. Query the table manually
with `FINAL` when comparing operational counts:

```sql
SELECT count()
FROM raw_logs FINAL
WHERE chain_id = 1 AND is_removed = false;
```

Useful PostgreSQL state while ClickHouse is unavailable:

```sql
SELECT id, collection_scope, current_block, status, error_message
FROM eventlake_subscriptions
WHERE status IN ('error', 'clickhouse_reorg_retrying');
```

## 8. Existing Deployments

The new migration `202608050001_raw_event_lake.sql` adds subscription scope and
makes `contract_address` nullable for `all_events`. It does not move historical
PostgreSQL raw logs into ClickHouse and it does not delete decoded history.

To retain historical raw data in ClickHouse, backfill it in a separately controlled
operation, verify per-chain/block/topic counts, then enable new collection. Do not
enable a full-chain subscription until the chosen RPC provider and block windows are
tested for its `eth_getLogs` limits.

## 9. Verification

```bash
cargo fmt --check
cargo check --locked --features clickhouse
cargo test --locked --features clickhouse
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true \
  cargo test --locked --features clickhouse --test clickhouse_integration_tests -- --nocapture
docker compose -f docker-compose.clickhouse.yml config --quiet
```
