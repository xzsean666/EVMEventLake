use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::error::ApplicationError,
};

pub fn routes() -> Router<ApplicationState> {
    Router::new().route("/api/dashboard", get(dashboard_summary))
}

#[derive(OpenApi)]
#[openapi(paths(dashboard_summary), components(schemas(DashboardSummary)))]
struct DashboardApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    DashboardApiDocumentation::openapi()
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct DashboardSummary {
    pub active_jobs: i64,
    pub paused_jobs: i64,
    pub errored_jobs: i64,
    pub total_raw_logs: i64,
    pub total_decoded_events: i64,
    pub healthy_rpc_endpoints: i64,
    pub unhealthy_rpc_endpoints: i64,
    pub active_block_sync_jobs: i64,
    pub errored_block_sync_jobs: i64,
}

#[utoipa::path(
    get,
    path = "/api/dashboard",
    tag = "dashboard",
    responses((status = 200, description = "Dashboard summary", body = ApiResponse<DashboardSummary>))
)]
async fn dashboard_summary(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<DashboardSummary>>, ApplicationError> {
    #[cfg(feature = "clickhouse")]
    let ch_enabled = state.configuration.clickhouse.enabled;
    #[cfg(not(feature = "clickhouse"))]
    let ch_enabled = false;

    let query_str = if ch_enabled {
        r#"
        SELECT
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE active = true) AS active_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'paused') AS paused_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'error') AS errored_jobs,
            0::BIGINT AS total_raw_logs,
            0::BIGINT AS total_decoded_events,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'healthy') AS healthy_rpc_endpoints,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'unhealthy') AS unhealthy_rpc_endpoints,
            (SELECT COUNT(*)::BIGINT FROM eventlake_block_transaction_sync_state WHERE status IN ('syncing', 'caught_up', 'realtime_syncing')) AS active_block_sync_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_block_transaction_sync_state WHERE status = 'error') AS errored_block_sync_jobs
        "#
    } else {
        r#"
        SELECT
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE active = true) AS active_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'paused') AS paused_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'error') AS errored_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_raw_logs WHERE removed = false) AS total_raw_logs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_decoded_events WHERE decode_status = 'decoded') AS total_decoded_events,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'healthy') AS healthy_rpc_endpoints,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'unhealthy') AS unhealthy_rpc_endpoints,
            (SELECT COUNT(*)::BIGINT FROM eventlake_block_transaction_sync_state WHERE status IN ('syncing', 'caught_up', 'realtime_syncing')) AS active_block_sync_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_block_transaction_sync_state WHERE status = 'error') AS errored_block_sync_jobs
        "#
    };

    let summary = sqlx::query_as::<_, DashboardSummary>(query_str)
        .fetch_one(&state.pool)
        .await?;

    #[cfg(feature = "clickhouse")]
    let summary = if state.configuration.clickhouse.enabled {
        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        let total_raw_logs = crate::clickhouse::raw_log_count(&client)
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!(
                    "ClickHouse dashboard query failed: {error}"
                ))
            })?;
        let total_decoded_events = crate::clickhouse::decoded_event_count(&client)
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!(
                    "ClickHouse dashboard query failed: {error}"
                ))
            })?;
        DashboardSummary {
            total_raw_logs,
            // This legacy field is preserved for historical pre-upgrade rows. New
            // raw-event-lake collection does not create decoded-event rows.
            total_decoded_events,
            ..summary
        }
    } else {
        summary
    };

    Ok(response::success(summary))
}
