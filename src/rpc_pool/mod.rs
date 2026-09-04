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
        .route(
            "/api/rpc-endpoints/{id}",
            get(get_rpc_endpoint).delete(delete_rpc_endpoint),
        )
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
        delete_rpc_endpoint,
        enable_rpc_endpoint,
        disable_rpc_endpoint,
        check_rpc_endpoint
    ),
    components(schemas(RpcEndpointRecord, CreateRpcEndpointRequest, RpcEndpointSeed))
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

#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
pub struct RpcEndpointSeed {
    pub chain_id: i64,
    pub url: String,
    #[serde(default)]
    pub weight: Option<i32>,
    #[serde(default)]
    pub chain_name: Option<String>,
    #[serde(default)]
    pub native_token_symbol: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RpcSeedsInput {
    List(Vec<RpcEndpointSeed>),
    Object { endpoints: Vec<RpcEndpointSeed> },
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
    delete,
    path = "/api/rpc-endpoints/{id}",
    tag = "rpc",
    params(("id" = uuid::Uuid, Path, description = "RPC endpoint id")),
    responses(
        (status = 200, description = "RPC endpoint deleted", body = ApiResponse<RpcEndpointRecord>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "RPC endpoint not found")
    )
)]
async fn delete_rpc_endpoint(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RpcEndpointRecord>>, ApplicationError> {
    principal.require_admin()?;
    let endpoint = sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        DELETE FROM eventlake_rpc_endpoints
        WHERE id = $1
        RETURNING id, chain_id, url, status, weight, latency_ms, last_check_at,
                  failure_count, last_error, created_at, updated_at
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("rpc endpoint {id}")))?;

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

    if !is_private_rpc_allowed() {
        validate_rpc_url_ssrf(&parsed_url)?;
    }

    Ok(())
}

fn is_private_rpc_allowed() -> bool {
    if cfg!(test) && std::env::var("EVENTLAKE_ENFORCE_SSRF_TEST").is_err() {
        return true;
    }
    std::env::var("EVENTLAKE_ALLOW_PRIVATE_RPC")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

fn validate_rpc_url_ssrf(parsed_url: &reqwest::Url) -> Result<(), ApplicationError> {
    if let Some(host_str) = parsed_url.host_str() {
        let lower = host_str.to_ascii_lowercase();
        if lower == "localhost"
            || lower.ends_with(".localhost")
            || lower.ends_with(".local")
            || lower.ends_with(".internal")
        {
            return Err(ApplicationError::BadRequest(
                "localhost or internal domain RPC endpoint is not allowed".to_owned(),
            ));
        }

        if let Ok(ip) = host_str.parse::<std::net::IpAddr>() {
            if is_private_ip(ip) {
                return Err(ApplicationError::BadRequest(
                    "private, loopback, or link-local RPC endpoint IP is not allowed".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

const SELECT_RPC_ENDPOINTS: &str = r#"
SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
       failure_count, last_error, created_at, updated_at
FROM eventlake_rpc_endpoints
ORDER BY chain_id, status, weight DESC, created_at
"#;

pub async fn seed_rpc_endpoints_from_file(
    pool: &sqlx::PgPool,
    path: &str,
) -> anyhow::Result<usize> {
    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() {
        tracing::warn!(path = %path, "RPC seeds file not found, skipping seeding");
        return Ok(0);
    }

    let file_content = tokio::fs::read_to_string(path_obj)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read RPC seeds file at {path}: {e}"))?;

    seed_rpc_endpoints_from_json(pool, &file_content).await
}

pub async fn seed_rpc_endpoints_from_json(
    pool: &sqlx::PgPool,
    json_str: &str,
) -> anyhow::Result<usize> {
    let seeds: Vec<RpcEndpointSeed> = match serde_json::from_str::<RpcSeedsInput>(json_str) {
        Ok(RpcSeedsInput::List(list)) => list,
        Ok(RpcSeedsInput::Object { endpoints }) => endpoints,
        Err(err) => {
            anyhow::bail!("failed to parse RPC seeds JSON: {err}");
        }
    };

    let mut seeded_count = 0;
    for seed in seeds {
        let url = seed.url.trim().to_owned();
        let weight = seed.weight.unwrap_or(100);

        if let Err(err) = validate_rpc_endpoint_request(seed.chain_id, &url, weight) {
            tracing::warn!(
                chain_id = seed.chain_id,
                url = %url,
                error = %err,
                "skipping invalid RPC seed entry"
            );
            continue;
        }

        let chain_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM eventlake_chains WHERE chain_id = $1)",
        )
        .bind(seed.chain_id)
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        if !chain_exists {
            let chain_name = seed
                .chain_name
                .unwrap_or_else(|| format!("Chain {}", seed.chain_id));
            let symbol = seed.native_token_symbol.unwrap_or_else(|| "ETH".to_owned());
            let _ = sqlx::query(
                r#"
                INSERT INTO eventlake_chains (
                    chain_id, name, native_token_symbol, safe_confirmation_depth,
                    default_min_block_window, default_max_block_window
                )
                VALUES ($1, $2, $3, 12, 1, 1000)
                ON CONFLICT (chain_id) DO NOTHING
                "#,
            )
            .bind(seed.chain_id)
            .bind(chain_name)
            .bind(symbol)
            .execute(pool)
            .await;
        }

        let result = sqlx::query(
            r#"
            INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, weight, status)
            VALUES ($1, $2, $3, $4, 'enabled')
            ON CONFLICT (chain_id, url) DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(seed.chain_id)
        .bind(url)
        .bind(weight)
        .execute(pool)
        .await;

        match result {
            Ok(res) => {
                if res.rows_affected() > 0 {
                    seeded_count += 1;
                }
            }
            Err(err) => {
                tracing::warn!(
                    chain_id = seed.chain_id,
                    url = %seed.url,
                    error = %err,
                    "failed to insert RPC endpoint seed"
                );
            }
        }
    }

    tracing::info!(seeded_count, "completed RPC endpoints seeding");
    Ok(seeded_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rpc_seeds_from_json_list() {
        let json_data = r#"[
            {
                "chain_id": 1,
                "url": "https://eth.llamarpc.com",
                "weight": 100
            },
            {
                "chain_id": 8453,
                "url": "https://mainnet.base.org",
                "chain_name": "Base",
                "native_token_symbol": "ETH"
            }
        ]"#;

        let parsed: RpcSeedsInput = serde_json::from_str(json_data).expect("parses list JSON");
        match parsed {
            RpcSeedsInput::List(seeds) => {
                assert_eq!(seeds.len(), 2);
                assert_eq!(seeds[0].chain_id, 1);
                assert_eq!(seeds[0].weight, Some(100));
                assert_eq!(seeds[1].chain_id, 8453);
                assert_eq!(seeds[1].weight, None);
                assert_eq!(seeds[1].chain_name.as_deref(), Some("Base"));
            }
            _ => panic!("expected List variant"),
        }
    }

    #[test]
    fn parses_rpc_seeds_from_json_object() {
        let json_data = r#"{
            "endpoints": [
                {
                    "chain_id": 56,
                    "url": "https://bsc-dataseed.binance.org",
                    "weight": 80
                }
            ]
        }"#;

        let parsed: RpcSeedsInput = serde_json::from_str(json_data).expect("parses object JSON");
        match parsed {
            RpcSeedsInput::Object { endpoints } => {
                assert_eq!(endpoints.len(), 1);
                assert_eq!(endpoints[0].chain_id, 56);
                assert_eq!(endpoints[0].weight, Some(80));
            }
            _ => panic!("expected Object variant"),
        }
    }

    #[test]
    fn validates_rpc_endpoint_requests() {
        assert!(validate_rpc_endpoint_request(1, "https://eth.llamarpc.com", 100).is_ok());
        assert!(validate_rpc_endpoint_request(1, "http://127.0.0.1:8545", 50).is_ok());
        assert!(validate_rpc_endpoint_request(0, "https://eth.llamarpc.com", 100).is_err());
        assert!(validate_rpc_endpoint_request(1, "https://eth.llamarpc.com", 0).is_err());
        assert!(validate_rpc_endpoint_request(1, "ftp://eth.llamarpc.com", 100).is_err());
        assert!(validate_rpc_endpoint_request(1, "not-a-url", 100).is_err());
    }
}
