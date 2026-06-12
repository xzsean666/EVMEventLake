ALTER TABLE eventlake_chains
    ADD CONSTRAINT eventlake_chains_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_chains_safe_depth_non_negative CHECK (safe_confirmation_depth >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_chains_status_known CHECK (status IN ('active', 'disabled')) NOT VALID;

ALTER TABLE eventlake_rpc_endpoints
    ADD CONSTRAINT eventlake_rpc_endpoints_weight_positive CHECK (weight > 0) NOT VALID,
    ADD CONSTRAINT eventlake_rpc_endpoints_latency_non_negative CHECK (latency_ms IS NULL OR latency_ms >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_rpc_endpoints_failure_count_non_negative CHECK (failure_count >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_rpc_endpoints_status_known CHECK (status IN ('enabled', 'disabled', 'healthy', 'unhealthy')) NOT VALID;

ALTER TABLE eventlake_abi_versions
    ADD CONSTRAINT eventlake_abi_versions_event_count_non_negative CHECK (event_count >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_abi_versions_status_known CHECK (status IN ('active', 'deleted')) NOT VALID;

ALTER TABLE eventlake_contract_registry
    ADD CONSTRAINT eventlake_contract_registry_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_contract_registry_event_count_non_negative CHECK (event_count >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_contract_registry_block_bounds CHECK (
        (first_seen_block IS NULL OR first_seen_block >= 0)
        AND (last_seen_block IS NULL OR last_seen_block >= 0)
        AND (
            first_seen_block IS NULL
            OR last_seen_block IS NULL
            OR last_seen_block >= first_seen_block
        )
    ) NOT VALID;

ALTER TABLE eventlake_subscriptions
    ADD CONSTRAINT eventlake_subscriptions_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_subscriptions_block_bounds CHECK (
        start_block >= 0
        AND current_block >= 0
        AND (target_block IS NULL OR target_block >= 0)
    ) NOT VALID,
    ADD CONSTRAINT eventlake_subscriptions_status_known CHECK (
        status IN (
            'pending',
            'historical_syncing',
            'historical_synced',
            'realtime_syncing',
            'paused',
            'deleted',
            'error'
        )
    ) NOT VALID;

ALTER TABLE eventlake_block_checkpoints
    ADD CONSTRAINT eventlake_block_checkpoints_block_non_negative CHECK (block_number >= 0) NOT VALID;

ALTER TABLE eventlake_raw_logs
    ADD CONSTRAINT eventlake_raw_logs_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_raw_logs_block_non_negative CHECK (block_number >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_raw_logs_transaction_index_non_negative CHECK (transaction_index >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_raw_logs_log_index_non_negative CHECK (log_index >= 0) NOT VALID;

ALTER TABLE eventlake_decode_queue
    ADD CONSTRAINT eventlake_decode_queue_block_non_negative CHECK (block_number >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_decode_queue_attempt_count_non_negative CHECK (attempt_count >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_decode_queue_status_known CHECK (status IN ('pending', 'decoded', 'error')) NOT VALID;

ALTER TABLE eventlake_decoded_events
    ADD CONSTRAINT eventlake_decoded_events_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_decoded_events_block_non_negative CHECK (block_number >= 0) NOT VALID,
    ADD CONSTRAINT eventlake_decoded_events_decode_status_known CHECK (decode_status IN ('decoded', 'reorged', 'error')) NOT VALID;

ALTER TABLE eventlake_address_index
    ADD CONSTRAINT eventlake_address_index_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_address_index_block_non_negative CHECK (block_number >= 0) NOT VALID;

ALTER TABLE eventlake_event_field_index
    ADD CONSTRAINT eventlake_event_field_index_chain_id_positive CHECK (chain_id > 0) NOT VALID,
    ADD CONSTRAINT eventlake_event_field_index_block_non_negative CHECK (block_number >= 0) NOT VALID;

ALTER TABLE eventlake_api_keys
    ADD CONSTRAINT eventlake_api_keys_role_known CHECK (role IN ('admin', 'read_only')) NOT VALID;
