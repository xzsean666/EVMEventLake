use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    chains,
    shared::error::ApplicationError,
};

pub mod evm_rpc_client;
pub mod worker;

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route(
            "/api/rpc-endpoints",
            get(list_rpc_endpoints).post(create_rpc_endpoint),
        )
        .route("/api/rpc-endpoints/{id}", get(get_rpc_endpoint))
        .route("/api/rpc-endpoints/{id}/enable", post(enable_rpc_endpoint))
        .route(
            "/api/rpc-endpoints/{id}/disable",
            post(disable_rpc_endpoint),
        )
        .route("/api/rpc-endpoints/{id}/check", post(check_rpc_endpoint))
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_rpc_endpoints,
        get_rpc_endpoint,
        create_rpc_endpoint,
        enable_rpc_endpoint,
        disable_rpc_endpoint,
        check_rpc_endpoint
    ),
    components(schemas(RpcEndpointRecord, CreateRpcEndpointRequest))
)]
struct RpcApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    RpcApiDocumentation::openapi()
}

#[derive(Debug, Serialize, FromRow, Clone, ToSchema)]
pub struct RpcEndpointRecord {
    pub id: Uuid,
    pub chain_id: i64,
    pub url: String,
    pub status: String,
    pub weight: i32,
    pub latency_ms: Option<i64>,
    pub last_check_at: Option<DateTime<Utc>>,
    pub failure_count: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRpcEndpointRequest {
    pub chain_id: i64,
    pub url: String,
    pub weight: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/rpc-endpoints",
    tag = "rpc",
    responses((status = 200, description = "RPC endpoints", body = ApiResponse<Vec<RpcEndpointRecord>>))
)]
async fn list_rpc_endpoints(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<RpcEndpointRecord>>>, ApplicationError> {
    let endpoints = sqlx::query_as::<_, RpcEndpointRecord>(SELECT_RPC_ENDPOINTS)
        .fetch_all(&state.pool)
        .await?;

    Ok(response::success(endpoints))
}

#[utoipa::path(
    get,
    path = "/api/rpc-endpoints/{id}",
    tag = "rpc",
    params(("id" = uuid::Uuid, Path, description = "RPC endpoint id")),
    responses(
        (status = 200, description = "RPC endpoint", body = ApiResponse<RpcEndpointRecord>),
        (status = 404, description = "RPC endpoint not found")
    )
)]
async fn get_rpc_endpoint(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    let endpoint = find_rpc_endpoint(&state.pool, id).await?;
    Ok(response::success(endpoint))
}

#[utoipa::path(
    post,
    path = "/api/rpc-endpoints",
    tag = "rpc",
    request_body = CreateRpcEndpointRequest,
    responses(
        (status = 200, description = "RPC endpoint created or updated", body = ApiResponse<RpcEndpointRecord>),
        (status = 400, description = "Invalid RPC endpoint request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
async fn create_rpc_endpoint(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateRpcEndpointRequest>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    principal.require_admin()?;

    let url = request.url.trim().to_owned();
    let weight = request.weight.unwrap_or(100);
    validate_rpc_endpoint_request(request.chain_id, &url, weight)?;
    chains::get_collection_policy(&state.pool, request.chain_id).await?;

    let endpoint = sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, weight)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (chain_id, url) DO UPDATE
        SET weight = EXCLUDED.weight,
            status = 'enabled',
            updated_at = now()
        RETURNING id, chain_id, url, status, weight, latency_ms, last_check_at,
                  failure_count, last_error, created_at, updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(request.chain_id)
    .bind(url)
    .bind(weight)
    .fetch_one(&state.pool)
    .await?;

    Ok(response::success(endpoint))
}

#[utoipa::path(
    post,
    path = "/api/rpc-endpoints/{id}/enable",
    tag = "rpc",
    params(("id" = uuid::Uuid, Path, description = "RPC endpoint id")),
    responses((status = 200, description = "RPC endpoint enabled", body = ApiResponse<RpcEndpointRecord>))
)]
async fn enable_rpc_endpoint(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    principal.require_admin()?;
    let endpoint = update_rpc_status(&state.pool, id, "enabled").await?;
    Ok(response::success(endpoint))
}

#[utoipa::path(
    post,
    path = "/api/rpc-endpoints/{id}/disable",
    tag = "rpc",
    params(("id" = uuid::Uuid, Path, description = "RPC endpoint id")),
    responses((status = 200, description = "RPC endpoint disabled", body = ApiResponse<RpcEndpointRecord>))
)]
async fn disable_rpc_endpoint(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    principal.require_admin()?;
    let endpoint = update_rpc_status(&state.pool, id, "disabled").await?;
    Ok(response::success(endpoint))
}

#[utoipa::path(
    post,
    path = "/api/rpc-endpoints/{id}/check",
    tag = "rpc",
    params(("id" = uuid::Uuid, Path, description = "RPC endpoint id")),
    responses(
        (status = 200, description = "RPC endpoint health checked", body = ApiResponse<RpcEndpointRecord>),
        (status = 502, description = "RPC endpoint check failed")
    )
)]
async fn check_rpc_endpoint(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    principal.require_admin()?;
    let endpoint = find_rpc_endpoint(&state.pool, id).await?;
    let check_result = evm_rpc_client::check_endpoint(&state.http_client, &endpoint.url).await;
    persist_health_check(&state.pool, endpoint.id, check_result).await?;
    let endpoint = find_rpc_endpoint(&state.pool, id).await?;
    Ok(response::success(endpoint))
}

pub async fn select_rpc_endpoint(
    pool: &sqlx::PgPool,
    chain_id: i64,
) -> Result<RpcEndpointRecord, ApplicationError> {
    sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
               failure_count, last_error, created_at, updated_at
        FROM eventlake_rpc_endpoints
        WHERE chain_id = $1 AND status IN ('enabled', 'healthy')
        ORDER BY failure_count ASC, weight DESC, latency_ms ASC NULLS LAST, updated_at ASC
        LIMIT 1
        "#,
    )
    .bind(chain_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("healthy RPC endpoint for chain {chain_id}")))
}

pub async fn mark_rpc_failure(
    pool: &sqlx::PgPool,
    id: Uuid,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_rpc_endpoints
        SET failure_count = failure_count + 1,
            last_error = $2,
            last_check_at = now(),
            status = CASE WHEN failure_count + 1 >= 3 THEN 'unhealthy' ELSE status END,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

async fn find_rpc_endpoint(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<RpcEndpointRecord, ApplicationError> {
    sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
               failure_count, last_error, created_at, updated_at
        FROM eventlake_rpc_endpoints
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("rpc endpoint {id}")))
}

async fn update_rpc_status(
    pool: &sqlx::PgPool,
    id: Uuid,
    status: &str,
) -> Result<RpcEndpointRecord, ApplicationError> {
    sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        UPDATE eventlake_rpc_endpoints
        SET status = $2,
            updated_at = now()
        WHERE id = $1
        RETURNING id, chain_id, url, status, weight, latency_ms, last_check_at,
                  failure_count, last_error, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(status)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("rpc endpoint {id}")))
}

async fn persist_health_check(
    pool: &sqlx::PgPool,
    id: Uuid,
    check_result: Result<evm_rpc_client::RpcHealthCheck, ApplicationError>,
) -> Result<(), ApplicationError> {
    match check_result {
        Ok(check) => {
            sqlx::query(
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
            .bind(id)
            .bind(check.latency_ms)
            .execute(pool)
            .await?;
        }
        Err(error) => {
            mark_rpc_failure(pool, id, &error.public_message()).await?;
        }
    }

    Ok(())
}

fn validate_rpc_endpoint_request(
    chain_id: i64,
    url: &str,
    weight: i32,
) -> Result<(), ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be greater than 0".to_owned(),
        ));
    }

    if weight <= 0 {
        return Err(ApplicationError::BadRequest(
            "weight must be greater than 0".to_owned(),
        ));
    }

    let parsed_url = reqwest::Url::parse(url)
        .map_err(|_| ApplicationError::BadRequest("url must be a valid URL".to_owned()))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(ApplicationError::BadRequest(
            "url must use http or https".to_owned(),
        ));
    }

    Ok(())
}

const SELECT_RPC_ENDPOINTS: &str = r#"
SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
       failure_count, last_error, created_at, updated_at
FROM eventlake_rpc_endpoints
ORDER BY chain_id, status, weight DESC, created_at
"#;
