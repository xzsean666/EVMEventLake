-- Support the event explorer's join on decoded events by topic0, which previously
-- forced a sequential scan over the whole decoded events table.
CREATE INDEX IF NOT EXISTS eventlake_decoded_events_topic0_idx
    ON eventlake_decoded_events(topic0);

-- Reorg invalidation flips decoded events to the 'reorged' status. The search path
-- only returns rows with decode_status = 'decoded', so this index keeps that filter
-- cheap as reorged rows accumulate.
CREATE INDEX IF NOT EXISTS eventlake_decoded_events_status_block_idx
    ON eventlake_decoded_events(decode_status, block_number DESC);
