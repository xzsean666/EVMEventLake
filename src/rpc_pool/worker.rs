use tokio::time::{MissedTickBehavior, interval};

use crate::{app::application_state::ApplicationState, rpc_pool};

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
        FROM rpc_endpoints
        WHERE status IN ('enabled', 'healthy', 'unhealthy')
        ORDER BY last_check_at ASC NULLS FIRST
        LIMIT 25
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    for endpoint in endpoints {
        let result =
            rpc_pool::evm_rpc_client::check_endpoint(&state.http_client, &endpoint.url).await;
        match result {
            Ok(check) => {
                sqlx::query(
                    r#"
                    UPDATE rpc_endpoints
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
                .execute(&state.pool)
                .await?;
            }
            Err(error) => {
                rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error.public_message())
                    .await?;
            }
        }
    }

    Ok(())
}
