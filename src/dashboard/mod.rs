use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::error::ApplicationError,
};

pub fn routes() -> Router<ApplicationState> {
    Router::new().route("/api/dashboard", get(dashboard_summary))
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
}

async fn dashboard_summary(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<DashboardSummary>>, ApplicationError> {
    let summary = sqlx::query_as::<_, DashboardSummary>(
        r#"
        SELECT
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE active = true) AS active_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'paused') AS paused_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_subscriptions WHERE status = 'error') AS errored_jobs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_raw_logs) AS total_raw_logs,
            (SELECT COUNT(*)::BIGINT FROM eventlake_decoded_events) AS total_decoded_events,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'healthy') AS healthy_rpc_endpoints,
            (SELECT COUNT(*)::BIGINT FROM eventlake_rpc_endpoints WHERE status = 'unhealthy') AS unhealthy_rpc_endpoints
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(response::success(summary))
}
