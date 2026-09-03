use std::collections::{HashMap, HashSet};

use tokio::time::{MissedTickBehavior, interval};
use uuid::Uuid;

use crate::{
    app::application_state::ApplicationState,
    chains,
    indexing::partition_manager,
    reorg,
    rpc_pool::{self, evm_rpc_client::RpcLog},
    shared::{error::ApplicationError, hex::parse_hex_u64, validation::normalize_address},
    subscriptions::{self, SubscriptionRecord},
};

const MAX_WINDOW_REDUCTION_ATTEMPTS: usize = 16;
const FAST_GROW_LOG_THRESHOLD: usize = 100;

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.worker_tick);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(error) = collect_once(&state).await {
            tracing::warn!(error = %error, "collector worker tick failed");
        }
    }
}

pub async fn collect_once(state: &ApplicationState) -> Result<(), ApplicationError> {
    let max_batch_size = state.configuration.background.max_batch_addresses.max(1);
    let candidate_limit = (max_batch_size * 4).max(50) as i64;
    let subscriptions = subscriptions::runnable_subscriptions(&state.pool, candidate_limit).await?;

    let buckets = bucket_subscriptions(subscriptions);

    for subscription in buckets.standalone {
        if let Err(error) = collect_subscription(state, &subscription).await {
            tracing::warn!(
                subscription_id = %subscription.id,
                error = %error,
                "subscription collection failed"
            );
            subscriptions::mark_subscription_error(
                &state.pool,
                subscription.id,
                &error.public_message(),
            )
            .await?;
        }
    }

    for ((_chain_id, _current_block), subs) in buckets.batched {
        for chunk in subs.chunks(max_batch_size) {
            if chunk.len() == 1 {
                if let Err(error) = collect_subscription(state, &chunk[0]).await {
                    tracing::warn!(
                        subscription_id = %chunk[0].id,
                        error = %error,
                        "subscription collection failed"
                    );
                    subscriptions::mark_subscription_error(
                        &state.pool,
                        chunk[0].id,
                        &error.public_message(),
                    )
                    .await?;
                }
            } else if let Err(error) = collect_subscription_batch(state, chunk).await {
                tracing::warn!(
                    batch_size = chunk.len(),
                    error = %error,
                    "subscription batch collection failed; falling back to individual"
                );
                // Graceful fallback: execute individually on failure
                for sub in chunk {
                    if let Err(sub_error) = collect_subscription(state, sub).await {
                        tracing::warn!(
                            subscription_id = %sub.id,
                            error = %sub_error,
                            "individual subscription fallback failed"
                        );
                        subscriptions::mark_subscription_error(
                            &state.pool,
                            sub.id,
                            &sub_error.public_message(),
                        )
                        .await?;
                    }
                }
            }
        }
    }

    Ok(())
}

pub(crate) struct SubscriptionBuckets {
    pub standalone: Vec<SubscriptionRecord>,
    pub batched: HashMap<(i64, i64), Vec<SubscriptionRecord>>,
}

pub(crate) fn bucket_subscriptions(subscriptions: Vec<SubscriptionRecord>) -> SubscriptionBuckets {
    let mut standalone = Vec::new();
    let mut batched: HashMap<(i64, i64), Vec<SubscriptionRecord>> = HashMap::new();

    for sub in subscriptions {
        if sub.collection_scope == "all_events"
            || sub.contract_address.is_none()
            || sub.status == "clickhouse_reorg_retrying"
        {
            standalone.push(sub);
        } else {
            batched
                .entry((sub.chain_id, sub.current_block))
                .or_default()
                .push(sub);
        }
    }

    SubscriptionBuckets {
        standalone,
        batched,
    }
}

async fn collect_subscription_batch(
    state: &ApplicationState,
    batch: &[SubscriptionRecord],
) -> Result<(), ApplicationError> {
    if batch.is_empty() {
        return Ok(());
    }
    if batch.len() == 1 {
        return collect_subscription(state, &batch[0]).await;
    }

    let chain_id = batch[0].chain_id;
    let from_block = batch[0].current_block;

    let collection_policy = chains::get_collection_policy(&state.pool, chain_id).await?;
    let endpoint = rpc_pool::select_rpc_endpoint(&state.pool, chain_id).await?;
    let chain_head =
        rpc_pool::evm_rpc_client::eth_block_number(&state.http_client, &endpoint.url).await?;
    let safe_head = chain_head.saturating_sub(collection_policy.safe_confirmation_depth);

    if from_block > safe_head {
        let updates: Vec<subscriptions::CheckpointBatchUpdate> = batch
            .iter()
            .map(|sub| {
                let status = if sub.realtime_enabled {
                    "realtime_syncing"
                } else {
                    "historical_synced"
                };
                let current_window = clamp_block_window(
                    sub.current_block_window,
                    sub.min_block_window,
                    sub.max_block_window,
                );
                subscriptions::CheckpointBatchUpdate {
                    id: sub.id,
                    next_block: sub.current_block,
                    target_block: Some(safe_head),
                    status: status.to_owned(),
                    current_block_window: current_window,
                }
            })
            .collect();
        subscriptions::update_checkpoints_batch(&state.pool, &updates).await?;
        return Ok(());
    }

    let min_batch_window = batch.iter().map(|s| s.min_block_window).max().unwrap_or(1);
    let mut block_window = batch
        .iter()
        .map(|s| {
            clamp_block_window(
                s.current_block_window,
                s.min_block_window,
                s.max_block_window,
            )
        })
        .min()
        .unwrap_or(1);

    let mut address_to_sub_id: HashMap<String, Uuid> = HashMap::with_capacity(batch.len());
    let mut addresses = Vec::with_capacity(batch.len());
    let batch_ids: Vec<Uuid> = batch.iter().map(|s| s.id).collect();

    for sub in batch {
        if let Some(addr) = &sub.contract_address {
            let normalized = normalize_address(addr)?;
            address_to_sub_id.insert(normalized.clone(), sub.id);
            addresses.push(normalized);
        }
    }

    let mut reduction_attempts = 0;

    loop {
        let to_block = block_range_end(from_block, block_window, safe_head);

        #[cfg(feature = "clickhouse")]
        let postgres_raw_storage = !state.configuration.clickhouse.enabled;
        #[cfg(not(feature = "clickhouse"))]
        let postgres_raw_storage = true;

        if postgres_raw_storage {
            partition_manager::ensure_partitions_for_range(&state.pool, from_block, to_block)
                .await?;
        }

        let logs_result = rpc_pool::evm_rpc_client::eth_get_logs(
            &state.http_client,
            &endpoint.url,
            &addresses,
            from_block,
            to_block,
        )
        .await;

        let logs = match logs_result {
            Ok(logs) => logs,
            Err(error) if is_get_logs_window_error(&error) && block_window > min_batch_window => {
                let next_window = shrink_block_window(block_window, min_batch_window);
                let error_message = error.public_message();
                tracing::info!(
                    batch_size = batch.len(),
                    chain_id,
                    from_block,
                    to_block,
                    previous_window = block_window,
                    next_window,
                    error = %error_message,
                    "shrinking eth_getLogs block window for batch"
                );
                subscriptions::update_collection_windows_after_retryable_error(
                    &state.pool,
                    &batch_ids,
                    next_window,
                    &error_message,
                )
                .await?;

                block_window = next_window;
                reduction_attempts += 1;
                if reduction_attempts >= MAX_WINDOW_REDUCTION_ATTEMPTS {
                    return Ok(());
                }
                continue;
            }
            Err(error) => {
                let error_message = error.public_message();
                if let Err(mark_error) =
                    rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await
                {
                    tracing::warn!(
                        endpoint_id = %endpoint.id,
                        error = %mark_error,
                        "failed to persist RPC failure"
                    );
                }
                return Err(error);
            }
        };

        let log_count = logs.len();

        let mut observed_blocks = HashSet::new();
        for log in &logs {
            let block_number = parse_hex_u64(&log.block_number)?;
            if observed_blocks.insert(block_number) {
                let result = reorg::observe_block_with_postgres_storage(
                    &state.pool,
                    chain_id,
                    block_number,
                    &log.block_hash,
                    postgres_raw_storage,
                    postgres_raw_storage,
                )
                .await?;
                if matches!(result, reorg::BlockCheckpointResult::ReorgDetected { .. }) {
                    #[cfg(feature = "clickhouse")]
                    if state.configuration.clickhouse.enabled {
                        let tombstone_result = match crate::clickhouse::active_client(state).await {
                            Ok(Some(client)) => crate::clickhouse::invalidate_from_block(
                                &client,
                                chain_id,
                                block_number,
                            )
                            .await
                            .map_err(|error| {
                                ApplicationError::ExternalService(format!(
                                    "ClickHouse reorg tombstone write failed: {error}"
                                ))
                            }),
                            Ok(None) => Err(ApplicationError::ExternalService(
                                "ClickHouse is enabled but no client is available".to_owned(),
                            )),
                            Err(error) => Err(error),
                        };
                        if let Err(error) = tombstone_result {
                            subscriptions::mark_chain_clickhouse_reorg_retrying(
                                &state.pool,
                                chain_id,
                                block_number,
                                &error.public_message(),
                            )
                            .await?;
                            tracing::error!(
                                chain_id,
                                from_block = block_number,
                                error = %error,
                                "ClickHouse reorg tombstone write failed; affected subscriptions will retry"
                            );
                            return Ok(());
                        }
                    }
                    tracing::warn!(
                        batch_size = batch.len(),
                        chain_id,
                        block_number,
                        "reorg detected during batch collection; aborting tick to re-collect"
                    );
                    return Ok(());
                }
            }
        }

        #[cfg(feature = "clickhouse")]
        if state.configuration.clickhouse.enabled {
            let client = crate::clickhouse::active_client(state)
                .await?
                .ok_or_else(|| {
                    ApplicationError::ExternalService(
                        "ClickHouse is enabled but no client is available".to_owned(),
                    )
                })?;
            let raw_logs = logs
                .iter()
                .map(|log| {
                    let addr = normalize_address(&log.address)?;
                    let sub_id = address_to_sub_id.get(&addr).copied();
                    clickhouse_raw_log(sub_id, chain_id, log)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Err(error) = crate::clickhouse::write_raw_logs(&client, &raw_logs).await {
                state.clear_clickhouse_client();
                return Err(ApplicationError::ExternalService(format!(
                    "ClickHouse raw-log write failed: {error}"
                )));
            }
        }

        if postgres_raw_storage {
            for log in &logs {
                let addr = normalize_address(&log.address)?;
                let sub_id = address_to_sub_id.get(&addr).copied();
                store_raw_log(&state.pool, sub_id, chain_id, log).await?;
            }
        }

        let next_block = to_block + 1;
        let updates: Vec<subscriptions::CheckpointBatchUpdate> = batch
            .iter()
            .map(|sub| {
                let status = if next_block > safe_head {
                    if sub.realtime_enabled {
                        "realtime_syncing"
                    } else {
                        "historical_synced"
                    }
                } else {
                    "historical_syncing"
                };
                let next_window = if reduction_attempts == 0 {
                    grow_block_window(sub.current_block_window, sub.max_block_window, log_count)
                } else {
                    block_window
                };
                subscriptions::CheckpointBatchUpdate {
                    id: sub.id,
                    next_block,
                    target_block: Some(safe_head),
                    status: status.to_owned(),
                    current_block_window: next_window,
                }
            })
            .collect();

        subscriptions::update_checkpoints_batch(&state.pool, &updates).await?;

        return Ok(());
    }
}

async fn collect_subscription(
    state: &ApplicationState,
    subscription: &SubscriptionRecord,
) -> Result<(), ApplicationError> {
    #[cfg(feature = "clickhouse")]
    if state.configuration.clickhouse.enabled && subscription.status == "clickhouse_reorg_retrying"
    {
        match crate::clickhouse::active_client(state)
            .await
            .and_then(|client| {
                client.ok_or_else(|| {
                    ApplicationError::ExternalService(
                        "ClickHouse is enabled but no client is available".to_owned(),
                    )
                })
            }) {
            Ok(client) => match crate::clickhouse::invalidate_from_block(
                &client,
                subscription.chain_id,
                subscription.current_block,
            )
            .await
            {
                Ok(()) => {
                    subscriptions::resume_after_clickhouse_reorg(&state.pool, subscription.id)
                        .await?;
                    tracing::info!(
                        subscription_id = %subscription.id,
                        chain_id = subscription.chain_id,
                        from_block = subscription.current_block,
                        "ClickHouse reorg tombstones applied; collection will resume"
                    );
                }
                Err(error) => {
                    tracing::error!(
                        subscription_id = %subscription.id,
                        chain_id = subscription.chain_id,
                        from_block = subscription.current_block,
                        error = %error,
                        "ClickHouse reorg tombstone retry failed"
                    );
                }
            },
            Err(error) => {
                tracing::error!(
                    subscription_id = %subscription.id,
                    chain_id = subscription.chain_id,
                    from_block = subscription.current_block,
                    error = %error,
                    "ClickHouse is unavailable while retrying a reorg"
                );
            }
        }
        return Ok(());
    }

    let collection_policy =
        chains::get_collection_policy(&state.pool, subscription.chain_id).await?;
    let endpoint = rpc_pool::select_rpc_endpoint(&state.pool, subscription.chain_id).await?;
    let chain_head =
        rpc_pool::evm_rpc_client::eth_block_number(&state.http_client, &endpoint.url).await?;
    let safe_head = chain_head.saturating_sub(collection_policy.safe_confirmation_depth);
    let current_window = clamp_block_window(
        subscription.current_block_window,
        subscription.min_block_window,
        subscription.max_block_window,
    );

    if subscription.current_block > safe_head {
        let status = if subscription.realtime_enabled {
            "realtime_syncing"
        } else {
            "historical_synced"
        };
        subscriptions::update_checkpoint(
            &state.pool,
            subscription.id,
            subscription.current_block,
            Some(safe_head),
            status,
            current_window,
        )
        .await?;
        return Ok(());
    }

    let from_block = subscription.current_block;
    let mut block_window = current_window;
    let mut reduction_attempts = 0;

    loop {
        let to_block = block_range_end(from_block, block_window, safe_head);
        // In ClickHouse mode raw logs never enter PostgreSQL, so PostgreSQL partition
        // maintenance is only needed for the PostgreSQL raw-log deployment.
        #[cfg(feature = "clickhouse")]
        let postgres_raw_storage = !state.configuration.clickhouse.enabled;
        #[cfg(not(feature = "clickhouse"))]
        let postgres_raw_storage = true;
        if postgres_raw_storage {
            partition_manager::ensure_partitions_for_range(&state.pool, from_block, to_block)
                .await?;
        }
        let addrs = subscription
            .contract_address
            .as_deref()
            .map(|addr| vec![addr])
            .unwrap_or_default();
        let logs_result = rpc_pool::evm_rpc_client::eth_get_logs(
            &state.http_client,
            &endpoint.url,
            &addrs,
            from_block,
            to_block,
        )
        .await;

        let logs = match logs_result {
            Ok(logs) => logs,
            Err(error)
                if is_get_logs_window_error(&error)
                    && block_window > subscription.min_block_window =>
            {
                let next_window = shrink_block_window(block_window, subscription.min_block_window);
                let error_message = error.public_message();
                tracing::info!(
                    subscription_id = %subscription.id,
                    chain_id = subscription.chain_id,
                    contract_address = ?subscription.contract_address,
                    from_block,
                    to_block,
                    previous_window = block_window,
                    next_window,
                    error = %error_message,
                    "shrinking eth_getLogs block window"
                );
                subscriptions::update_collection_window_after_retryable_error(
                    &state.pool,
                    subscription.id,
                    next_window,
                    &error_message,
                )
                .await?;

                block_window = next_window;
                reduction_attempts += 1;
                if reduction_attempts >= MAX_WINDOW_REDUCTION_ATTEMPTS {
                    return Ok(());
                }
                continue;
            }
            Err(error) => {
                let error_message = error.public_message();
                if let Err(mark_error) =
                    rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await
                {
                    tracing::warn!(
                        endpoint_id = %endpoint.id,
                        error = %mark_error,
                        "failed to persist RPC failure"
                    );
                }
                return Err(error);
            }
        };

        let log_count = logs.len();

        // Observe each block once (logs share a block hash within a block) to detect
        // reorgs cheaply. If the chain reorganised, `observe_block` has already
        // invalidated the affected range and rewound this subscription, so we abort and
        // let the next tick re-collect the canonical fork instead of advancing.
        let mut observed_blocks = HashSet::new();
        for log in &logs {
            let block_number = parse_hex_u64(&log.block_number)?;
            if observed_blocks.insert(block_number) {
                let result = reorg::observe_block_with_postgres_storage(
                    &state.pool,
                    subscription.chain_id,
                    block_number,
                    &log.block_hash,
                    postgres_raw_storage,
                    postgres_raw_storage,
                )
                .await?;
                if matches!(result, reorg::BlockCheckpointResult::ReorgDetected { .. }) {
                    #[cfg(feature = "clickhouse")]
                    if state.configuration.clickhouse.enabled {
                        let tombstone_result = match crate::clickhouse::active_client(state).await {
                            Ok(Some(client)) => crate::clickhouse::invalidate_from_block(
                                &client,
                                subscription.chain_id,
                                block_number,
                            )
                            .await
                            .map_err(|error| {
                                ApplicationError::ExternalService(format!(
                                    "ClickHouse reorg tombstone write failed: {error}"
                                ))
                            }),
                            Ok(None) => Err(ApplicationError::ExternalService(
                                "ClickHouse is enabled but no client is available".to_owned(),
                            )),
                            Err(error) => Err(error),
                        };
                        if let Err(error) = tombstone_result {
                            subscriptions::mark_chain_clickhouse_reorg_retrying(
                                &state.pool,
                                subscription.chain_id,
                                block_number,
                                &error.public_message(),
                            )
                            .await?;
                            tracing::error!(
                                chain_id = subscription.chain_id,
                                from_block = block_number,
                                error = %error,
                                "ClickHouse reorg tombstone write failed; affected subscriptions will retry"
                            );
                            return Ok(());
                        }
                    }
                    tracing::warn!(
                        subscription_id = %subscription.id,
                        chain_id = subscription.chain_id,
                        block_number,
                        "reorg detected during collection; aborting tick to re-collect"
                    );
                    return Ok(());
                }
            }
        }

        #[cfg(feature = "clickhouse")]
        if state.configuration.clickhouse.enabled {
            let client = crate::clickhouse::active_client(state)
                .await?
                .ok_or_else(|| {
                    ApplicationError::ExternalService(
                        "ClickHouse is enabled but no client is available".to_owned(),
                    )
                })?;
            let raw_logs = logs
                .iter()
                .map(|log| clickhouse_raw_log(Some(subscription.id), subscription.chain_id, log))
                .collect::<Result<Vec<_>, _>>()?;
            if let Err(error) = crate::clickhouse::write_raw_logs(&client, &raw_logs).await {
                // The checkpoint is deliberately not advanced. On the next worker tick we
                // fetch and write the same range again; ReplacingMergeTree makes that retry
                // idempotent at the `(chain, block, tx, log_index)` key.
                state.clear_clickhouse_client();
                return Err(ApplicationError::ExternalService(format!(
                    "ClickHouse raw-log write failed: {error}"
                )));
            }
        }

        if postgres_raw_storage {
            for log in &logs {
                store_raw_log(
                    &state.pool,
                    Some(subscription.id),
                    subscription.chain_id,
                    log,
                )
                .await?;
            }
        }

        let next_block = to_block + 1;
        let status = if next_block > safe_head {
            "realtime_syncing"
        } else {
            "historical_syncing"
        };
        let next_window = if reduction_attempts == 0 {
            grow_block_window(block_window, subscription.max_block_window, log_count)
        } else {
            block_window
        };

        subscriptions::update_checkpoint(
            &state.pool,
            subscription.id,
            next_block,
            Some(safe_head),
            status,
            next_window,
        )
        .await?;

        return Ok(());
    }
}

async fn store_raw_log(
    pool: &sqlx::PgPool,
    subscription_id: Option<Uuid>,
    chain_id: i64,
    log: &RpcLog,
) -> Result<(), ApplicationError> {
    let block_number = parse_hex_u64(&log.block_number)?;
    let transaction_index = parse_hex_u64(&log.transaction_index)?;
    let log_index = parse_hex_u64(&log.log_index)?;
    let contract_address = normalize_address(&log.address)?;
    let topics = normalize_topics(&log.topics)?;
    let topics_value = serde_json::to_value(&topics)
        .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;

    // PostgreSQL-only mode still retains raw logs, but raw-event-lake mode has no local
    // decode queue. A ClickHouse deployment bypasses this function completely.
    sqlx::query(
        r#"
        INSERT INTO eventlake_raw_logs (
            id, subscription_id, chain_id, contract_address, block_number, block_hash,
            transaction_hash, transaction_index, log_index, topics, data, removed
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (chain_id, transaction_hash, log_index, block_number) DO UPDATE
        SET removed = false,
            block_hash = EXCLUDED.block_hash,
            data = EXCLUDED.data,
            topics = EXCLUDED.topics,
            ingested_at = now()
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(subscription_id)
    .bind(chain_id)
    .bind(contract_address)
    .bind(block_number)
    .bind(&log.block_hash)
    .bind(&log.transaction_hash)
    .bind(transaction_index)
    .bind(log_index)
    .bind(topics_value)
    .bind(&log.data)
    .bind(log.removed.unwrap_or(false))
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(feature = "clickhouse")]
fn clickhouse_raw_log(
    subscription_id: Option<Uuid>,
    chain_id: i64,
    log: &RpcLog,
) -> Result<crate::clickhouse::RawLog, ApplicationError> {
    let topics = normalize_topics(&log.topics)?;
    Ok(crate::clickhouse::RawLog {
        id: Uuid::new_v4(),
        subscription_id,
        chain_id,
        block_number: parse_hex_u64(&log.block_number)?,
        block_hash: log.block_hash.clone(),
        transaction_hash: log.transaction_hash.clone(),
        transaction_index: parse_hex_u64(&log.transaction_index)?,
        log_index: parse_hex_u64(&log.log_index)?,
        contract_address: normalize_address(&log.address)?,
        topics,
        data: log.data.clone(),
        is_removed: log.removed.unwrap_or(false),
    })
}

fn normalize_topics(topics: &[String]) -> Result<Vec<String>, ApplicationError> {
    topics
        .iter()
        .map(|topic| crate::shared::validation::normalize_topic(topic))
        .collect()
}

fn block_range_end(from_block: i64, block_window: i64, safe_head: i64) -> i64 {
    from_block
        .saturating_add(block_window)
        .saturating_sub(1)
        .min(safe_head)
}

fn clamp_block_window(current: i64, min_window: i64, max_window: i64) -> i64 {
    current.max(min_window).min(max_window)
}

fn shrink_block_window(current: i64, min_window: i64) -> i64 {
    (current / 2).max(min_window)
}

fn grow_block_window(current: i64, max_window: i64, log_count: usize) -> i64 {
    if current >= max_window {
        return max_window;
    }

    let increment = if log_count <= FAST_GROW_LOG_THRESHOLD {
        current
    } else {
        (current / 4).max(1)
    };

    current.saturating_add(increment).min(max_window)
}

fn is_get_logs_window_error(error: &ApplicationError) -> bool {
    let message = error.public_message().to_ascii_lowercase();

    if message.contains("rate limit")
        || message.contains("too many requests")
        || message.contains("429")
    {
        return false;
    }

    message.contains("log response size exceeded")
        || message.contains("response size exceeded")
        || message.contains("too many results")
        || message.contains("too many logs")
        || (message.contains("more than")
            && (message.contains("result") || message.contains("log")))
        || message.contains("block range")
        || message.contains("range too")
        || message.contains("range is too")
        || message.contains("range limit")
        || message.contains("exceed maximum")
        || message.contains("exceeds maximum")
        || message.contains("please narrow")
        || message.contains("query timeout")
        || message.contains("context deadline exceeded")
        || message.contains("gateway timeout")
        || message.contains("timed out")
        || message.contains("timeout")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_block_window_respects_minimum() {
        assert_eq!(shrink_block_window(1000, 10), 500);
        assert_eq!(shrink_block_window(11, 10), 10);
        assert_eq!(shrink_block_window(1, 1), 1);
    }

    #[test]
    fn grow_block_window_expands_light_contracts_faster() {
        assert_eq!(grow_block_window(100, 1000, 0), 200);
        assert_eq!(grow_block_window(200, 1000, FAST_GROW_LOG_THRESHOLD), 400);
        assert_eq!(
            grow_block_window(400, 1000, FAST_GROW_LOG_THRESHOLD + 1),
            500
        );
        assert_eq!(grow_block_window(1000, 1000, 0), 1000);
    }

    #[test]
    fn get_logs_window_errors_are_classified_without_rate_limits() {
        let range_error =
            ApplicationError::ExternalService("query returned more than 10000 results".to_owned());
        assert!(is_get_logs_window_error(&range_error));

        let timeout_error = ApplicationError::ExternalService("504 Gateway Timeout".to_owned());
        assert!(is_get_logs_window_error(&timeout_error));

        let rate_limit_error = ApplicationError::ExternalService(
            "429 too many requests: rate limit exceeded".to_owned(),
        );
        assert!(!is_get_logs_window_error(&rate_limit_error));
    }

    fn make_test_sub(
        chain_id: i64,
        current_block: i64,
        contract_address: Option<&str>,
        scope: &str,
    ) -> SubscriptionRecord {
        SubscriptionRecord {
            id: Uuid::new_v4(),
            chain_id,
            contract_address: contract_address.map(|s| s.to_owned()),
            collection_scope: scope.to_owned(),
            abi_id: None,
            start_block: 100,
            current_block,
            target_block: None,
            min_block_window: 10,
            max_block_window: 1000,
            current_block_window: 100,
            status: "realtime_syncing".to_owned(),
            realtime_enabled: true,
            active: true,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn bucket_subscriptions_groups_by_chain_and_block_and_isolates_standalone() {
        let sub1 = make_test_sub(
            1,
            1000,
            Some("0x1111111111111111111111111111111111111111"),
            "contract",
        );
        let sub2 = make_test_sub(
            1,
            1000,
            Some("0x2222222222222222222222222222222222222222"),
            "contract",
        );
        let sub3 = make_test_sub(
            1,
            1005,
            Some("0x3333333333333333333333333333333333333333"),
            "contract",
        );
        let sub4 = make_test_sub(
            8453,
            1000,
            Some("0x4444444444444444444444444444444444444444"),
            "contract",
        );
        let sub_all_events = make_test_sub(1, 1000, None, "all_events");

        let buckets = bucket_subscriptions(vec![
            sub1.clone(),
            sub2.clone(),
            sub3.clone(),
            sub4.clone(),
            sub_all_events.clone(),
        ]);

        assert_eq!(buckets.standalone.len(), 1);
        assert_eq!(buckets.standalone[0].collection_scope, "all_events");

        assert_eq!(buckets.batched.len(), 3);
        // (chain 1, block 1000) should have 2 subs: sub1 & sub2
        let group_1_1000 = buckets
            .batched
            .get(&(1, 1000))
            .expect("bucket (1, 1000) exists");
        assert_eq!(group_1_1000.len(), 2);

        // (chain 1, block 1005) should have 1 sub: sub3
        let group_1_1005 = buckets
            .batched
            .get(&(1, 1005))
            .expect("bucket (1, 1005) exists");
        assert_eq!(group_1_1005.len(), 1);

        // (chain 8453, block 1000) should have 1 sub: sub4
        let group_8453_1000 = buckets
            .batched
            .get(&(8453, 1000))
            .expect("bucket (8453, 1000) exists");
        assert_eq!(group_8453_1000.len(), 1);
    }
}
