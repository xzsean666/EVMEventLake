-- Raw-event-lake mode keeps PostgreSQL for operational state only.  A subscription
-- can now either filter one contract or collect every EVM log from its chain.
ALTER TABLE eventlake_subscriptions
    ALTER COLUMN contract_address DROP NOT NULL,
    ADD COLUMN IF NOT EXISTS collection_scope TEXT NOT NULL DEFAULT 'contract';

ALTER TABLE eventlake_subscriptions
    DROP CONSTRAINT IF EXISTS eventlake_subscriptions_collection_scope_known;

ALTER TABLE eventlake_subscriptions
    ADD CONSTRAINT eventlake_subscriptions_collection_scope_known CHECK (
        collection_scope IN ('contract', 'all_events')
    ) NOT VALID;

ALTER TABLE eventlake_subscriptions
    DROP CONSTRAINT IF EXISTS eventlake_subscriptions_scope_address_consistent;

ALTER TABLE eventlake_subscriptions
    ADD CONSTRAINT eventlake_subscriptions_scope_address_consistent CHECK (
        (collection_scope = 'contract' AND contract_address IS NOT NULL)
        OR (collection_scope = 'all_events' AND contract_address IS NULL)
    ) NOT VALID;

DROP INDEX IF EXISTS eventlake_subscriptions_one_active_contract_idx;

CREATE UNIQUE INDEX IF NOT EXISTS eventlake_subscriptions_one_active_scope_idx
    ON eventlake_subscriptions (chain_id, collection_scope, COALESCE(contract_address, ''))
    WHERE active = true;

-- PostgreSQL raw mode stays useful for small deployments and needs the same query
-- shape as ClickHouse raw search: chain/block range followed by positional topics.
CREATE INDEX IF NOT EXISTS eventlake_raw_logs_chain_block_active_idx
    ON eventlake_raw_logs (chain_id, block_number DESC, log_index DESC)
    WHERE removed = false;

CREATE INDEX IF NOT EXISTS eventlake_raw_logs_topic0_active_idx
    ON eventlake_raw_logs ((topics ->> 0))
    WHERE removed = false;

CREATE INDEX IF NOT EXISTS eventlake_raw_logs_topic1_active_idx
    ON eventlake_raw_logs ((topics ->> 1))
    WHERE removed = false;

CREATE INDEX IF NOT EXISTS eventlake_raw_logs_topic2_active_idx
    ON eventlake_raw_logs ((topics ->> 2))
    WHERE removed = false;

CREATE INDEX IF NOT EXISTS eventlake_raw_logs_topic3_active_idx
    ON eventlake_raw_logs ((topics ->> 3))
    WHERE removed = false;
