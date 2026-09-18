-- EVMEventLake Baseline Schema Migration (SQLite Metadata Engine)
-- Consolidated operational state schema for chains, RPC pool, subscriptions, checkpoints, API keys, and block/tx sync.
-- Raw event logs and block/tx analytics are exclusively handled by ClickHouse.

-- 1. Supported EVM Chains
CREATE TABLE eventlake_chains (
    chain_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    native_token_symbol TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    safe_confirmation_depth INTEGER NOT NULL DEFAULT 12,
    default_min_block_window INTEGER NOT NULL DEFAULT 1,
    default_max_block_window INTEGER NOT NULL DEFAULT 1000,
    rpc_notes TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (chain_id > 0),
    CHECK (safe_confirmation_depth >= 0),
    CHECK (status IN ('active', 'disabled')),
    CHECK (
        default_min_block_window >= 1
        AND default_max_block_window >= default_min_block_window
    )
);

INSERT INTO eventlake_chains (
    chain_id,
    name,
    native_token_symbol,
    safe_confirmation_depth,
    default_min_block_window,
    default_max_block_window,
    rpc_notes
) VALUES
    (1, 'Ethereum', 'ETH', 12, 1, 1000, 'Mainnet execution JSON-RPC'),
    (8453, 'Base', 'ETH', 60, 1, 1000, 'OP Stack L2'),
    (42161, 'Arbitrum', 'ETH', 60, 1, 1000, 'Arbitrum One L2'),
    (10, 'Optimism', 'ETH', 60, 1, 1000, 'OP Mainnet L2'),
    (137, 'Polygon', 'POL', 256, 1, 1000, 'Polygon Pos'),
    (56, 'BSC', 'BNB', 30, 1, 1000, 'BSC public endpoints may limit eth_getLogs')
ON CONFLICT (chain_id) DO NOTHING;

-- 2. RPC Endpoints Pool
CREATE TABLE eventlake_rpc_endpoints (
    id TEXT PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES eventlake_chains(chain_id),
    url TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled',
    weight INTEGER NOT NULL DEFAULT 100,
    latency_ms INTEGER,
    last_check_at DATETIME,
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (chain_id, url),
    CHECK (weight > 0),
    CHECK (latency_ms IS NULL OR latency_ms >= 0),
    CHECK (failure_count >= 0),
    CHECK (status IN ('enabled', 'disabled', 'healthy', 'unhealthy'))
);

CREATE INDEX eventlake_rpc_endpoints_chain_status_idx ON eventlake_rpc_endpoints(chain_id, status, weight DESC);

-- 3. Subscriptions (Contract-specific or Full-chain All-events)
CREATE TABLE eventlake_subscriptions (
    id TEXT PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES eventlake_chains(chain_id),
    contract_address TEXT,
    collection_scope TEXT NOT NULL DEFAULT 'contract',
    abi_id TEXT,
    start_block INTEGER NOT NULL,
    current_block INTEGER NOT NULL,
    target_block INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    realtime_enabled INTEGER NOT NULL DEFAULT 1,
    active INTEGER NOT NULL DEFAULT 1,
    min_block_window INTEGER NOT NULL DEFAULT 1,
    max_block_window INTEGER NOT NULL DEFAULT 1000,
    current_block_window INTEGER NOT NULL DEFAULT 1000,
    error_message TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (chain_id > 0),
    CHECK (
        start_block >= 0
        AND current_block >= 0
        AND (target_block IS NULL OR target_block >= 0)
    ),
    CHECK (
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
    ),
    CHECK (
        collection_scope IN ('contract', 'all_events')
    ),
    CHECK (
        (collection_scope = 'contract' AND contract_address IS NOT NULL)
        OR (collection_scope = 'all_events' AND contract_address IS NULL)
    ),
    CHECK (
        min_block_window >= 1
        AND max_block_window >= min_block_window
        AND current_block_window >= min_block_window
        AND current_block_window <= max_block_window
    )
);

CREATE UNIQUE INDEX eventlake_subscriptions_one_active_scope_idx
    ON eventlake_subscriptions (chain_id, collection_scope, COALESCE(contract_address, ''))
    WHERE active = 1;

CREATE INDEX eventlake_subscriptions_status_idx ON eventlake_subscriptions(status, active);

-- 4. Reorg Block Checkpoints
CREATE TABLE eventlake_block_checkpoints (
    chain_id INTEGER NOT NULL REFERENCES eventlake_chains(chain_id),
    block_number INTEGER NOT NULL,
    block_hash TEXT NOT NULL,
    observed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (chain_id, block_number),
    CHECK (block_number >= 0)
);

-- 5. API Keys Authentication
CREATE TABLE eventlake_api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME,
    CHECK (role IN ('admin', 'read_only'))
);

-- 6. Block and Transaction Sync Operational State
CREATE TABLE eventlake_block_transaction_sync_state (
    chain_id INTEGER PRIMARY KEY REFERENCES eventlake_chains(chain_id) ON DELETE CASCADE,
    next_block INTEGER NOT NULL,
    start_block INTEGER NOT NULL DEFAULT 0,
    safe_head INTEGER,
    latest_seen_block INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    realtime_enabled INTEGER NOT NULL DEFAULT 1,
    batch_size INTEGER NOT NULL DEFAULT 10,
    max_concurrency INTEGER NOT NULL DEFAULT 2,
    reorg_window INTEGER NOT NULL DEFAULT 32,
    last_error TEXT,
    last_success_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (next_block >= 0),
    CHECK (start_block >= 0),
    CHECK (batch_size >= 1 AND batch_size <= 500),
    CHECK (max_concurrency >= 1 AND max_concurrency <= 32),
    CHECK (reorg_window >= 0 AND reorg_window <= 1024),
    CHECK (
        status IN ('pending', 'syncing', 'caught_up', 'error', 'paused', 'reorg_retrying')
    )
);

CREATE INDEX eventlake_bt_sync_status_idx
    ON eventlake_block_transaction_sync_state (status, updated_at);
