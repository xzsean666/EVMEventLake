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
