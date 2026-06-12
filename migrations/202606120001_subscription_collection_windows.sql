ALTER TABLE eventlake_chains
    ADD COLUMN default_min_block_window BIGINT NOT NULL DEFAULT 1;

ALTER TABLE eventlake_chains
    ADD CONSTRAINT eventlake_chains_block_window_bounds
    CHECK (
        default_min_block_window >= 1
        AND default_max_block_window >= default_min_block_window
    );

ALTER TABLE eventlake_subscriptions
    ADD COLUMN min_block_window BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN max_block_window BIGINT NOT NULL DEFAULT 1000,
    ADD COLUMN current_block_window BIGINT NOT NULL DEFAULT 1000;

UPDATE eventlake_subscriptions subscription
SET min_block_window = chain.default_min_block_window,
    max_block_window = chain.default_max_block_window,
    current_block_window = chain.default_max_block_window
FROM eventlake_chains chain
WHERE chain.chain_id = subscription.chain_id;

ALTER TABLE eventlake_subscriptions
    ADD CONSTRAINT eventlake_subscriptions_block_window_bounds
    CHECK (
        min_block_window >= 1
        AND max_block_window >= min_block_window
        AND current_block_window >= min_block_window
        AND current_block_window <= max_block_window
    );
