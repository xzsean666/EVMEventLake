-- Block and transaction synchronization state table.
-- PostgreSQL holds operational state, checkpoint, and coordination for canonical block/transaction collection.
CREATE TABLE IF NOT EXISTS eventlake_block_transaction_sync_state (
    chain_id BIGINT PRIMARY KEY REFERENCES eventlake_chains(chain_id) ON DELETE CASCADE,
    next_block BIGINT NOT NULL,
    start_block BIGINT NOT NULL DEFAULT 0,
    safe_head BIGINT,
    latest_seen_block BIGINT,
    status TEXT NOT NULL DEFAULT 'pending',
    realtime_enabled BOOLEAN NOT NULL DEFAULT true,
    batch_size INTEGER NOT NULL DEFAULT 10,
    max_concurrency INTEGER NOT NULL DEFAULT 2,
    reorg_window INTEGER NOT NULL DEFAULT 32,
    last_error TEXT,
    last_success_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT eventlake_bt_sync_next_block_non_negative CHECK (next_block >= 0),
    CONSTRAINT eventlake_bt_sync_start_block_non_negative CHECK (start_block >= 0),
    CONSTRAINT eventlake_bt_sync_batch_size_range CHECK (batch_size >= 1 AND batch_size <= 500),
    CONSTRAINT eventlake_bt_sync_max_concurrency_range CHECK (max_concurrency >= 1 AND max_concurrency <= 32),
    CONSTRAINT eventlake_bt_sync_reorg_window_range CHECK (reorg_window >= 0 AND reorg_window <= 1024),
    CONSTRAINT eventlake_bt_sync_status_valid CHECK (
        status IN ('pending', 'syncing', 'caught_up', 'error', 'paused', 'reorg_retrying')
    )
);

CREATE INDEX IF NOT EXISTS eventlake_bt_sync_status_idx
    ON eventlake_block_transaction_sync_state (status, updated_at);
