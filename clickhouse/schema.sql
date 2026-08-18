CREATE TABLE IF NOT EXISTS decoded_events (
    id UUID,
    raw_log_id UUID,
    subscription_id Nullable(UUID),
    chain_id UInt64,
    block_number UInt64,
    block_hash String,
    transaction_hash String,
    log_index UInt32,
    contract_address String,
    event_name String,
    topic0 String,
    abi_id Nullable(UUID),
    indexed_fields String,
    non_indexed_fields String,
    decoded_fields String,
    is_removed Bool DEFAULT false,
    decoded_at DateTime64(3, 'UTC'),
    indexed_at DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(indexed_at)
PARTITION BY (chain_id, toYYYYMM(toDateTime(intDiv(block_number, 5) + 1438300000)))
ORDER BY (chain_id, block_number, log_index);

CREATE TABLE IF NOT EXISTS address_index (
    chain_id UInt64,
    address String,
    block_number UInt64,
    transaction_hash String,
    log_index UInt32,
    event_name String,
    contract_address String,
    role String,
    field_name String,
    is_removed Bool DEFAULT false,
    indexed_at DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (chain_id, address, block_number, transaction_hash, log_index, field_name);

CREATE TABLE IF NOT EXISTS event_field_index (
    chain_id UInt64,
    topic0 String,
    field_name String,
    field_value String,
    block_number UInt64,
    transaction_hash String,
    log_index UInt32,
    is_removed Bool DEFAULT false,
    indexed_at DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (
    chain_id, topic0, field_name, field_value, block_number, transaction_hash, log_index
);

-- Raw logs are the primary event-lake dataset. ReplacingMergeTree gives collector
-- retries and reorg tombstones a deterministic latest version for each EVM log.
CREATE TABLE IF NOT EXISTS raw_logs (
    id UUID,
    subscription_id Nullable(UUID),
    chain_id UInt64,
    block_number UInt64,
    block_hash String,
    transaction_hash String,
    transaction_index UInt32,
    log_index UInt32,
    contract_address String,
    topic0 String DEFAULT '',
    topic1 String DEFAULT '',
    topic2 String DEFAULT '',
    topic3 String DEFAULT '',
    topics String,
    data String,
    is_removed Bool DEFAULT false,
    ingested_at DateTime64(3, 'UTC'),
    stored_at DateTime64(3, 'UTC'),
    INDEX raw_logs_topic0_idx topic0 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX raw_logs_topic1_idx topic1 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX raw_logs_topic2_idx topic2 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX raw_logs_topic3_idx topic3 TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree(stored_at)
PARTITION BY chain_id
ORDER BY (chain_id, block_number, transaction_hash, log_index)
SETTINGS index_granularity = 8192;

-- Blocks are the primary block dataset. ReplacingMergeTree gives collector
-- retries and reorg tombstones a deterministic latest version for each EVM block.
CREATE TABLE IF NOT EXISTS blocks (
    chain_id UInt64,
    block_number UInt64,
    block_hash String,
    parent_hash String,
    timestamp UInt64,
    gas_limit String,
    gas_used String,
    base_fee_per_gas Nullable(String),
    beneficiary Nullable(String),
    transactions_root Nullable(String),
    receipts_root Nullable(String),
    state_root Nullable(String),
    size Nullable(String),
    withdrawals_root Nullable(String),
    blob_gas_used Nullable(String),
    excess_blob_gas Nullable(String),
    parent_beacon_block_root Nullable(String),
    transaction_count UInt32,
    is_canonical Bool DEFAULT true,
    stored_at DateTime64(3, 'UTC'),
    INDEX blocks_hash_idx block_hash TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree(stored_at)
PARTITION BY chain_id
ORDER BY (chain_id, block_number)
SETTINGS index_granularity = 8192;

-- Transactions dataset. ReplacingMergeTree gives collector retries and
-- reorg tombstones a deterministic latest version for each EVM transaction.
CREATE TABLE IF NOT EXISTS transactions (
    chain_id UInt64,
    tx_hash String,
    block_number UInt64,
    transaction_index UInt32,
    from_address String,
    to_address Nullable(String),
    value String,
    nonce String,
    gas String,
    gas_price Nullable(String),
    max_fee_per_gas Nullable(String),
    max_priority_fee_per_gas Nullable(String),
    tx_type Nullable(UInt32),
    method_id Nullable(String),
    is_canonical Bool DEFAULT true,
    stored_at DateTime64(3, 'UTC'),
    INDEX transactions_hash_idx tx_hash TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX transactions_from_idx from_address TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX transactions_to_idx to_address TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree(stored_at)
PARTITION BY chain_id
ORDER BY (chain_id, block_number, transaction_index, tx_hash)
SETTINGS index_granularity = 8192;

