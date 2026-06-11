use axum::{Json, Router, routing::get};
use serde_json::Value;
use utoipa::OpenApi;

use crate::{
    abi_registry, api::response, app::application_state::ApplicationState, auth, chains, dashboard,
    explorers, rpc_pool, search, subscriptions,
};

#[derive(OpenApi)]
#[openapi(
    info(title = "EventLake API", version = "1.0.0"),
    tags(
        (name = "health", description = "Service health endpoints"),
        (name = "chains", description = "Chain metadata endpoints"),
        (name = "rpc", description = "RPC pool endpoints"),
        (name = "abis", description = "ABI registry endpoints"),
        (name = "subscriptions", description = "Contract subscription endpoints"),
        (name = "search", description = "Unified search endpoint"),
        (name = "explorers", description = "Address, contract, and event explorer endpoints"),
        (name = "dashboard", description = "Operational dashboard endpoint")
    )
)]
struct ApiDocumentation;

pub fn build_router(state: ApplicationState) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .route("/api/openapi.json", get(openapi_document))
        .merge(auth::routes())
        .merge(chains::routes())
        .merge(rpc_pool::routes())
        .merge(abi_registry::routes())
        .merge(subscriptions::routes())
        .merge(search::routes())
        .merge(explorers::routes())
        .merge(dashboard::routes())
        .with_state(state)
}

async fn liveness() -> Json<response::ApiResponse<Value>> {
    response::success(serde_json::json!({ "status": "alive" }))
}

async fn readiness(
    axum::extract::State(state): axum::extract::State<ApplicationState>,
) -> Result<Json<response::ApiResponse<Value>>, crate::shared::error::ApplicationError> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(response::success(serde_json::json!({ "status": "ready" })))
}

async fn openapi_document() -> Json<Value> {
    let document = ApiDocumentation::openapi();
    Json(serde_json::to_value(document).expect("OpenAPI document serializes"))
}
