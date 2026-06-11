CREATE TABLE chains (
    chain_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    native_token_symbol TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    safe_confirmation_depth BIGINT NOT NULL DEFAULT 12,
    default_max_block_window BIGINT NOT NULL DEFAULT 1000,
    rpc_notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO chains (
    chain_id,
    name,
    native_token_symbol,
    safe_confirmation_depth,
    default_max_block_window,
    rpc_notes
) VALUES
    (1, 'Ethereum', 'ETH', 12, 1000, 'Mainnet execution JSON-RPC'),
    (8453, 'Base', 'ETH', 60, 1000, 'OP Stack L2'),
    (42161, 'Arbitrum', 'ETH', 60, 1000, 'Arbitrum One L2'),
    (10, 'Optimism', 'ETH', 60, 1000, 'OP Mainnet L2'),
    (137, 'Polygon', 'POL', 256, 1000, 'Polygon PoS'),
    (56, 'BSC', 'BNB', 30, 1000, 'BSC public endpoints may limit eth_getLogs')
ON CONFLICT (chain_id) DO NOTHING;

CREATE TABLE rpc_endpoints (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL REFERENCES chains(chain_id),
    url TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled',
    weight INTEGER NOT NULL DEFAULT 100,
    latency_ms BIGINT,
    last_check_at TIMESTAMPTZ,
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (chain_id, url)
);

CREATE INDEX rpc_endpoints_chain_status_idx ON rpc_endpoints(chain_id, status, weight DESC);

CREATE TABLE abi_versions (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    abi_json JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    event_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (name, version)
);

CREATE TABLE event_registry (
    id UUID PRIMARY KEY,
    abi_id UUID NOT NULL REFERENCES abi_versions(id),
    event_name TEXT NOT NULL,
    signature TEXT NOT NULL,
    topic0 TEXT NOT NULL,
    inputs JSONB NOT NULL,
    indexed_inputs JSONB NOT NULL,
    anonymous BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (abi_id, topic0)
);

CREATE INDEX event_registry_topic0_idx ON event_registry(topic0);
CREATE INDEX event_registry_name_idx ON event_registry(event_name);

CREATE TABLE contract_registry (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL REFERENCES chains(chain_id),
    contract_address TEXT NOT NULL,
    abi_id UUID REFERENCES abi_versions(id),
    event_count BIGINT NOT NULL DEFAULT 0,
    first_seen_block BIGINT,
    last_seen_block BIGINT,
    first_seen_at TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (chain_id, contract_address)
);

CREATE TABLE subscriptions (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL REFERENCES chains(chain_id),
    contract_address TEXT NOT NULL,
    abi_id UUID REFERENCES abi_versions(id),
    start_block BIGINT NOT NULL,
    current_block BIGINT NOT NULL,
    target_block BIGINT,
    status TEXT NOT NULL DEFAULT 'pending',
    realtime_enabled BOOLEAN NOT NULL DEFAULT true,
    active BOOLEAN NOT NULL DEFAULT true,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX subscriptions_one_active_contract_idx
    ON subscriptions(chain_id, contract_address)
    WHERE active = true;

CREATE INDEX subscriptions_status_idx ON subscriptions(status, active);

CREATE TABLE block_checkpoints (
    chain_id BIGINT NOT NULL REFERENCES chains(chain_id),
    block_number BIGINT NOT NULL,
    block_hash TEXT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chain_id, block_number)
);

CREATE TABLE raw_logs (
    id UUID NOT NULL,
    subscription_id UUID REFERENCES subscriptions(id),
    chain_id BIGINT NOT NULL,
    contract_address TEXT NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash TEXT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index BIGINT NOT NULL DEFAULT 0,
    log_index BIGINT NOT NULL,
    topics JSONB NOT NULL,
    data TEXT NOT NULL,
    removed BOOLEAN NOT NULL DEFAULT false,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id, block_number)
) PARTITION BY RANGE (block_number);

CREATE TABLE raw_logs_default PARTITION OF raw_logs DEFAULT;

CREATE UNIQUE INDEX raw_logs_unique_log_idx
    ON raw_logs(chain_id, transaction_hash, log_index, block_number);

CREATE INDEX raw_logs_chain_contract_block_idx
    ON raw_logs(chain_id, contract_address, block_number DESC);

CREATE TABLE decode_queue (
    id UUID PRIMARY KEY,
    raw_log_id UUID NOT NULL,
    block_number BIGINT NOT NULL,
    subscription_id UUID REFERENCES subscriptions(id),
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(raw_log_id, block_number)
);

CREATE INDEX decode_queue_status_idx ON decode_queue(status, created_at);

CREATE TABLE decoded_events (
    id UUID NOT NULL,
    raw_log_id UUID NOT NULL,
    block_number BIGINT NOT NULL,
    chain_id BIGINT NOT NULL,
    contract_address TEXT NOT NULL,
    abi_id UUID,
    event_name TEXT NOT NULL,
    topic0 TEXT NOT NULL,
    indexed_fields JSONB NOT NULL,
    non_indexed_fields JSONB NOT NULL,
    decode_status TEXT NOT NULL DEFAULT 'decoded',
    decode_error TEXT,
    decoded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id, block_number)
) PARTITION BY RANGE (block_number);

CREATE TABLE decoded_events_default PARTITION OF decoded_events DEFAULT;

CREATE UNIQUE INDEX decoded_events_raw_log_idx
    ON decoded_events(raw_log_id, block_number);

CREATE INDEX decoded_events_chain_contract_block_idx
    ON decoded_events(chain_id, contract_address, block_number DESC);

CREATE INDEX decoded_events_event_name_idx
    ON decoded_events(event_name, block_number DESC);

CREATE TABLE address_index (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    address TEXT NOT NULL,
    contract_address TEXT NOT NULL,
    event_name TEXT NOT NULL,
    field_name TEXT NOT NULL,
    raw_log_id UUID NOT NULL,
    block_number BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(chain_id, address, raw_log_id, field_name, block_number)
);

CREATE INDEX address_index_lookup_idx
    ON address_index(chain_id, address, block_number DESC);

CREATE TABLE event_field_index (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    contract_address TEXT NOT NULL,
    event_name TEXT NOT NULL,
    field_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    normalized_value TEXT NOT NULL,
    raw_log_id UUID NOT NULL,
    block_number BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(chain_id, contract_address, event_name, field_name, normalized_value, raw_log_id, block_number)
);

CREATE INDEX event_field_index_lookup_idx
    ON event_field_index(chain_id, event_name, field_name, normalized_value, block_number DESC);

CREATE TABLE api_keys (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);

