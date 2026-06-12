use serde_json::Value;
use tokio::time::{MissedTickBehavior, interval};
use uuid::Uuid;

use crate::{
    app::application_state::ApplicationState,
    chains, reorg,
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
    let subscriptions = subscriptions::runnable_subscriptions(&state.pool, 10).await?;

    for subscription in subscriptions {
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

    Ok(())
}

async fn collect_subscription(
    state: &ApplicationState,
    subscription: &SubscriptionRecord,
) -> Result<(), ApplicationError> {
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
        let logs_result = rpc_pool::evm_rpc_client::eth_get_logs(
            &state.http_client,
            &endpoint.url,
            &subscription.contract_address,
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
                    contract_address = %subscription.contract_address,
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
        for log in logs {
            store_raw_log(&state.pool, subscription, &log).await?;
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
    subscription: &SubscriptionRecord,
    log: &RpcLog,
) -> Result<(), ApplicationError> {
    let block_number = parse_hex_u64(&log.block_number)?;
    let transaction_index = parse_hex_u64(&log.transaction_index)?;
    let log_index = parse_hex_u64(&log.log_index)?;
    let contract_address = normalize_address(&log.address)?;
    let topics = normalize_topics(&log.topics)?;
    let topics_value = serde_json::to_value(&topics)
        .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;

    reorg::observe_block(pool, subscription.chain_id, block_number, &log.block_hash).await?;

    let raw_log_id = sqlx::query_as::<_, (Uuid,)>(
        r#"
        WITH inserted AS (
            INSERT INTO eventlake_raw_logs (
                id, subscription_id, chain_id, contract_address, block_number, block_hash,
                transaction_hash, transaction_index, log_index, topics, data, removed
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (chain_id, transaction_hash, log_index, block_number) DO NOTHING
            RETURNING id
        )
        SELECT id FROM inserted
        UNION ALL
        SELECT id FROM eventlake_raw_logs
        WHERE chain_id = $3
          AND transaction_hash = $7
          AND log_index = $9
          AND block_number = $5
        LIMIT 1
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(subscription.id)
    .bind(subscription.chain_id)
    .bind(contract_address)
    .bind(block_number)
    .bind(&log.block_hash)
    .bind(&log.transaction_hash)
    .bind(transaction_index)
    .bind(log_index)
    .bind(topics_value)
    .bind(&log.data)
    .bind(log.removed.unwrap_or(false))
    .fetch_one(pool)
    .await?
    .0;

    sqlx::query(
        r#"
        INSERT INTO eventlake_decode_queue (id, raw_log_id, block_number, subscription_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (raw_log_id, block_number) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(raw_log_id)
    .bind(block_number)
    .bind(subscription.id)
    .execute(pool)
    .await?;

    Ok(())
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

#[allow(dead_code)]
fn topics_as_value(topics: &[String]) -> Value {
    serde_json::to_value(topics).unwrap_or_else(|_| serde_json::json!([]))
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
}
