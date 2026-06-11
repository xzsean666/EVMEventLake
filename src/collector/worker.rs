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
    let (safe_confirmation_depth, max_block_window) =
        chains::get_collection_policy(&state.pool, subscription.chain_id).await?;
    let endpoint = rpc_pool::select_rpc_endpoint(&state.pool, subscription.chain_id).await?;
    let chain_head =
        rpc_pool::evm_rpc_client::eth_block_number(&state.http_client, &endpoint.url).await?;
    let safe_head = chain_head.saturating_sub(safe_confirmation_depth);

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
        )
        .await?;
        return Ok(());
    }

    let from_block = subscription.current_block;
    let to_block = (from_block + max_block_window - 1).min(safe_head);
    let logs = rpc_pool::evm_rpc_client::eth_get_logs(
        &state.http_client,
        &endpoint.url,
        &subscription.contract_address,
        from_block,
        to_block,
    )
    .await
    .inspect_err(|error| {
        let error_message = error.public_message();
        tokio::spawn({
            let pool = state.pool.clone();
            let endpoint_id = endpoint.id;
            async move {
                let _ = rpc_pool::mark_rpc_failure(&pool, endpoint_id, &error_message).await;
            }
        });
    })?;

    for log in logs {
        store_raw_log(&state.pool, subscription, &log).await?;
    }

    let next_block = to_block + 1;
    let status = if next_block > safe_head {
        "realtime_syncing"
    } else {
        "historical_syncing"
    };

    subscriptions::update_checkpoint(
        &state.pool,
        subscription.id,
        next_block,
        Some(safe_head),
        status,
    )
    .await?;

    Ok(())
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
            INSERT INTO raw_logs (
                id, subscription_id, chain_id, contract_address, block_number, block_hash,
                transaction_hash, transaction_index, log_index, topics, data, removed
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (chain_id, transaction_hash, log_index, block_number) DO NOTHING
            RETURNING id
        )
        SELECT id FROM inserted
        UNION ALL
        SELECT id FROM raw_logs
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
        INSERT INTO decode_queue (id, raw_log_id, block_number, subscription_id)
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

#[allow(dead_code)]
fn topics_as_value(topics: &[String]) -> Value {
    serde_json::to_value(topics).unwrap_or_else(|_| serde_json::json!([]))
}
