# ClickHouse Upgrade

Version: 1.2

Status: Current implementation

Date: 2026-08-04

## 1. Goal

ClickHouse is an optional scale-out search store. It is intended for large event
volumes and multi-filter analytical queries, not for subscriptions, RPC management,
authentication, or queues.

The upgrade deliberately does not keep a second copy of every decoded event and its
search indexes.

| Data or responsibility | PostgreSQL-only mode | ClickHouse mode |
| --- | --- | --- |
| Chain, RPC, ABI, contract metadata | PostgreSQL | PostgreSQL |
| Subscription, checkpoint, decode queue, auth | PostgreSQL | PostgreSQL |
| Raw EVM log | PostgreSQL | PostgreSQL |
| Decoded event | PostgreSQL | ClickHouse only |
| Address index | PostgreSQL | ClickHouse only |
| Custom event-field index | PostgreSQL | ClickHouse only |
| Search and explorer event statistics | PostgreSQL | ClickHouse |

PostgreSQL raw logs are retained as the source record so ABI fixes and re-decoding
remain possible. They are encoded chain logs, not a duplicate decoded-event/index
projection. Raw-log retention still consumes PostgreSQL disk; this upgrade removes
the duplicate *derived* data, not the source record.

## 2. Storage Mode

The runtime flag selects one mode for the process:

```env
# Default: PostgreSQL stores the derived event/search tables.
EVENTLAKE_CLICKHOUSE_ENABLED=false

# Large-data mode: ClickHouse stores those tables instead.
EVENTLAKE_CLICKHOUSE_ENABLED=true
```

The enabled mode also requires a binary built with the Cargo feature:

```bash
cargo build --release --locked --features clickhouse
```

The source Docker deployment is:

```bash
docker compose --env-file .env -f docker-compose.clickhouse.yml up -d --build
```

The client uses ClickHouse HTTP on port `8123`.

## 3. Write and Retry Semantics

In ClickHouse mode the decoder follows this sequence:

```text
RPC log -> PostgreSQL raw_log + decode_queue commit
        -> ABI decode
        -> ClickHouse decoded_events + address_index + event_field_index
        -> mark PostgreSQL queue entry decoded
```

The queue item is acknowledged only after the ClickHouse writes complete. A failed
write is logged, the queue entry becomes `clickhouse_retrying`, and the subscription
becomes `clickhouse_write_retrying`. The decoder retries on later worker ticks with
no attempt limit for this storage failure. Once all ClickHouse-retrying entries for
that subscription succeed, collection resumes automatically.

This means a ClickHouse outage pauses affected subscriptions rather than silently
losing data or allowing PostgreSQL and ClickHouse to diverge. A failed client is
discarded so the next retry establishes a fresh connection and reapplies the
idempotent schema DDL.

For a chain reorganization, PostgreSQL first marks raw logs removed and rewinds the
affected subscriptions. ClickHouse then writes `is_removed=true` tombstone versions.
If that write fails, collection remains in `clickhouse_reorg_retrying` until it
succeeds. Queries use `FINAL`, so tombstones are visible before background merges.

## 4. Search Behaviour

In PostgreSQL-only mode, `/api/search` and explorers use PostgreSQL's decoded-event,
address, and field index tables. In ClickHouse mode they use ClickHouse tables:

- `decoded_events FINAL`
- `address_index FINAL`
- `event_field_index FINAL`

This includes custom `field.<name>` filters, address filters, transaction hashes,
block ranges, event names, contracts, topics, sorting, the address explorer, contract
explorer, event explorer, and the dashboard's decoded-event total. Event definitions,
ABI metadata, and contract existence are still read from PostgreSQL because they are
small operational metadata.

There is intentionally no PostgreSQL fallback while ClickHouse mode is selected. A
fallback would either serve incomplete data or require retaining a duplicate derived
dataset. A ClickHouse search outage returns a service error while the writer retries.

## 5. ClickHouse Tables

`clickhouse/schema.sql` creates these `ReplacingMergeTree(indexed_at)` tables:

- `decoded_events`, ordered by `(chain_id, block_number, log_index)`.
- `address_index`, ordered by address and event position.
- `event_field_index`, ordered by chain, topic, field name/value, and event position.

The two secondary tables make a query such as "a user's Transfer events with custom
field filters" an indexed ClickHouse semi-join instead of a scan of decoded JSON.

## 6. Existing Deployments

This change prevents new duplicate derived writes. It does not automatically migrate
or delete a pre-existing PostgreSQL `eventlake_decoded_events`,
`eventlake_address_index`, or `eventlake_event_field_index` dataset.

For an already-running PostgreSQL-only deployment, perform a controlled backfill from
the retained raw logs, verify ClickHouse counts and representative searches, then
remove the old PostgreSQL derived projection in a separately approved maintenance
operation. Do not set `EVENTLAKE_CLICKHOUSE_ENABLED=true` on a live historical
dataset until that backfill is complete: new decoded data will go only to ClickHouse.

## 7. Operational Checks

```bash
curl -fsS http://127.0.0.1:8080/health/ready
docker compose --env-file .env -f docker-compose.clickhouse.yml logs -f eventlake
docker compose --env-file .env -f docker-compose.clickhouse.yml logs -f clickhouse
```

Useful PostgreSQL states during an outage:

```sql
SELECT id, status, current_block, error_message
FROM eventlake_subscriptions
WHERE status LIKE 'clickhouse_%';

SELECT status, COUNT(*)
FROM eventlake_decode_queue
GROUP BY status;
```

The migration `202608040001_clickhouse_retry_statuses.sql` adds the durable retry
statuses to PostgreSQL constraints.

## 8. Verification Commands

```bash
cargo fmt --check
cargo check --locked --features clickhouse
cargo test --locked --features clickhouse
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true \
  cargo test --locked --features clickhouse --test clickhouse_integration_tests -- --nocapture
docker compose -f docker-compose.clickhouse.yml config --quiet
```
