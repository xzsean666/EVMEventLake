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
    let summary = sqlx::query_as::<_, DashboardSummary>(
        r#"
        SELECT
            (SELECT COUNT(*) FROM eventlake_subscriptions WHERE active = 1) AS active_jobs,
            (SELECT COUNT(*) FROM eventlake_subscriptions WHERE status = 'paused') AS paused_jobs,
            (SELECT COUNT(*) FROM eventlake_subscriptions WHERE status = 'error') AS errored_jobs,
            0 AS total_raw_logs,
            0 AS total_decoded_events,
            (SELECT COUNT(*) FROM eventlake_rpc_endpoints WHERE status = 'healthy') AS healthy_rpc_endpoints,
            (SELECT COUNT(*) FROM eventlake_rpc_endpoints WHERE status = 'unhealthy') AS unhealthy_rpc_endpoints,
            (SELECT COUNT(*) FROM eventlake_block_transaction_sync_state WHERE status IN ('syncing', 'caught_up', 'realtime_syncing')) AS active_block_sync_jobs,
            (SELECT COUNT(*) FROM eventlake_block_transaction_sync_state WHERE status = 'error') AS errored_block_sync_jobs
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    let total_raw_logs = if let Some(client) = crate::clickhouse::active_client(&state).await? {
        crate::clickhouse::raw_log_count(&client)
            .await
            .unwrap_or(0)
    } else {
        0
    };

    let summary = DashboardSummary {
        total_raw_logs,
        ..summary
    };

    Ok(response::success(summary))
}
