-- ClickHouse mode keeps decoded events and their search indexes out of PostgreSQL.
-- These durable statuses block collection while the ClickHouse writer retries.
ALTER TABLE eventlake_subscriptions
    DROP CONSTRAINT IF EXISTS eventlake_subscriptions_status_known;

ALTER TABLE eventlake_subscriptions
    ADD CONSTRAINT eventlake_subscriptions_status_known CHECK (
        status IN (
            'pending',
            'historical_syncing',
            'historical_synced',
            'realtime_syncing',
            'paused',
            'deleted',
            'error',
            'clickhouse_write_retrying',
            'clickhouse_reorg_retrying'
        )
    ) NOT VALID;

ALTER TABLE eventlake_decode_queue
    DROP CONSTRAINT IF EXISTS eventlake_decode_queue_status_known;

ALTER TABLE eventlake_decode_queue
    ADD CONSTRAINT eventlake_decode_queue_status_known CHECK (
        status IN ('pending', 'decoded', 'error', 'clickhouse_retrying')
    ) NOT VALID;
