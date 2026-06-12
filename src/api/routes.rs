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
    paths(liveness, readiness, openapi_document),
    components(schemas(response::ApiResponse<serde_json::Value>, response::ApiErrorBody)),
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

#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses((status = 200, description = "Service is alive", body = response::ApiResponse<serde_json::Value>))
)]
async fn liveness() -> Json<response::ApiResponse<Value>> {
    response::success(serde_json::json!({ "status": "alive" }))
}

#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Service is ready", body = response::ApiResponse<serde_json::Value>),
        (status = 500, description = "Service is not ready", body = response::ApiResponse<serde_json::Value>)
    )
)]
async fn readiness(
    axum::extract::State(state): axum::extract::State<ApplicationState>,
) -> Result<Json<response::ApiResponse<Value>>, crate::shared::error::ApplicationError> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(response::success(serde_json::json!({ "status": "ready" })))
}

#[utoipa::path(
    get,
    path = "/api/openapi.json",
    tag = "health",
    responses((status = 200, description = "OpenAPI document", body = serde_json::Value))
)]
async fn openapi_document() -> Json<Value> {
    let mut document = ApiDocumentation::openapi();
    document.merge(auth::openapi());
    document.merge(chains::openapi());
    document.merge(rpc_pool::openapi());
    document.merge(abi_registry::openapi());
    document.merge(subscriptions::openapi());
    document.merge(search::openapi());
    document.merge(explorers::openapi());
    document.merge(dashboard::openapi());
    Json(serde_json::to_value(document).expect("OpenAPI document serializes"))
}
