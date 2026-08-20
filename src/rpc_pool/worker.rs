use std::{sync::Arc, time::Duration};

use tokio::{
    sync::Semaphore,
    task::JoinSet,
    time::{MissedTickBehavior, interval, timeout},
};

use crate::{app::application_state::ApplicationState, rpc_pool};

const MAX_CONCURRENT_CHECKS: usize = 20;
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(6);

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.worker_tick);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(error) = check_enabled_endpoints(&state).await {
            tracing::warn!(error = %error, "rpc health worker tick failed");
        }
    }
}

async fn check_enabled_endpoints(
    state: &ApplicationState,
) -> Result<(), crate::shared::error::ApplicationError> {
    let endpoints = sqlx::query_as::<_, rpc_pool::RpcEndpointRecord>(
        r#"
        SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
               failure_count, last_error, created_at, updated_at
        FROM eventlake_rpc_endpoints
        WHERE status IN ('enabled', 'healthy', 'unhealthy')
        ORDER BY last_check_at ASC NULLS FIRST
        LIMIT 100
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    if endpoints.is_empty() {
        return Ok(());
    }

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS));
    let mut set = JoinSet::new();

    for endpoint in endpoints {
        let sem = semaphore.clone();
        let client = state.http_client.clone();
        let pool = state.pool.clone();

        set.spawn(async move {
            let _permit = sem.acquire().await;
            let check_future = rpc_pool::evm_rpc_client::check_endpoint(&client, &endpoint.url);

            match timeout(HEALTH_CHECK_TIMEOUT, check_future).await {
                Ok(Ok(check)) => {
                    let _ = sqlx::query(
                        r#"
                        UPDATE eventlake_rpc_endpoints
                        SET status = 'healthy',
                            latency_ms = $2,
                            last_check_at = now(),
                            failure_count = 0,
                            last_error = NULL,
                            updated_at = now()
                        WHERE id = $1
                        "#,
                    )
                    .bind(endpoint.id)
                    .bind(check.latency_ms)
                    .execute(&pool)
                    .await;
                }
                Ok(Err(error)) => {
                    let _ = rpc_pool::mark_rpc_failure(&pool, endpoint.id, &error.public_message())
                        .await;
                }
                Err(_) => {
                    let _ = rpc_pool::mark_rpc_failure(
                        &pool,
                        endpoint.id,
                        &format!(
                            "health check timed out after {}s",
                            HEALTH_CHECK_TIMEOUT.as_secs()
                        ),
                    )
                    .await;
                }
            }
        });
    }

    while let Some(res) = set.join_next().await {
        if let Err(err) = res {
            tracing::warn!(error = %err, "rpc check task panicked or canceled");
        }
    }

    Ok(())
}
