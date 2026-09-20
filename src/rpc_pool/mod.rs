use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

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

/// Progressive backoff cooldown in seconds:
/// 1 failure: 60s (1m)
/// 2 failures: 300s (5m)
/// 3 failures: 900s (15m)
/// 4 failures: 3600s (1h)
/// 5 failures: 14400s (4h)
/// 6+ failures: 86400s (24h max)
pub fn calculate_cooldown_seconds(consecutive_failures: u32) -> u64 {
    match consecutive_failures {
        0 => 0,
        1 => 60,
        2 => 300,
        3 => 900,
        4 => 3600,
        5 => 14400,
        _ => 86400,
    }
}

#[derive(Debug, Clone, Default)]
pub struct EndpointRuntimeStatus {
    pub consecutive_failures: u32,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub current_weight: i32,
}

static ENDPOINT_RUNTIME_STATES: LazyLock<RwLock<HashMap<Uuid, EndpointRuntimeStatus>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Selects an RPC endpoint from a non-empty list of candidate records using
/// the Smooth Weighted Round-Robin (SWRR) algorithm.
///
/// Each step:
/// 1. For each candidate i: current_weight[i] += effective_weight[i]
/// 2. Pick candidate k with the maximum current_weight[k]
/// 3. current_weight[k] -= total_weight
///
/// This provides a perfectly smooth, interleaved distribution without clustering
/// and ensures all healthy endpoints receive traffic proportional to their configured weight.
pub fn select_weighted_round_robin(candidates: &[RpcEndpointRecord]) -> RpcEndpointRecord {
    if candidates.is_empty() {
        panic!("select_weighted_round_robin called with empty candidates");
    }
    if candidates.len() == 1 {
        return candidates[0].clone();
    }

    let mut cache = ENDPOINT_RUNTIME_STATES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let total_weight: i32 = candidates
        .iter()
        .map(|ep| ep.weight.max(1))
        .fold(0i32, |acc, w| acc.saturating_add(w));

    let mut best_id = candidates[0].id;
    let mut best_weight = i32::MIN;

    for ep in candidates {
        let entry = cache.entry(ep.id).or_default();
        let effective_weight = ep.weight.max(1);
        entry.current_weight = entry.current_weight.saturating_add(effective_weight);
        if entry.current_weight > best_weight {
            best_weight = entry.current_weight;
            best_id = ep.id;
        }
    }

    if let Some(selected_entry) = cache.get_mut(&best_id) {
        selected_entry.current_weight = selected_entry.current_weight.saturating_sub(total_weight);
    }

    candidates
        .iter()
        .find(|ep| ep.id == best_id)
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

pub fn get_cooldown_info(
    endpoint_id: &Uuid,
    now: DateTime<Utc>,
) -> (Option<DateTime<Utc>>, Option<i64>) {
    let cache = ENDPOINT_RUNTIME_STATES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(status) = cache.get(endpoint_id) {
        if let Some(until) = status.cooldown_until {
            if until > now {
                let remaining = (until - now).num_seconds().max(0);
                return (Some(until), Some(remaining));
            }
        }
    }
    (None, None)
}

pub fn get_endpoint_runtime_status(endpoint_id: &Uuid) -> EndpointRuntimeStatus {
    let cache = ENDPOINT_RUNTIME_STATES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.get(endpoint_id).cloned().unwrap_or_default()
}

pub fn reset_endpoint_cooldown_for_test() {
    let mut cache = ENDPOINT_RUNTIME_STATES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.clear();
}

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
    #[sqlx(default)]
    pub cooldown_until: Option<DateTime<Utc>>,
    #[sqlx(default)]
    pub cooldown_remaining_seconds: Option<i64>,
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
    let mut endpoints = sqlx::query_as::<_, RpcEndpointRecord>(SELECT_RPC_ENDPOINTS)
        .fetch_all(&state.pool)
        .await?;

    let now = Utc::now();
    for ep in &mut endpoints {
        let (until, remaining) = get_cooldown_info(&ep.id, now);
        ep.cooldown_until = until;
        ep.cooldown_remaining_seconds = remaining;
    }

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
    let mut endpoint = find_rpc_endpoint(&state.pool, id).await?;
    let (until, remaining) = get_cooldown_info(&endpoint.id, Utc::now());
    endpoint.cooldown_until = until;
    endpoint.cooldown_remaining_seconds = remaining;
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
            updated_at = CURRENT_TIMESTAMP
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
    pool: &sqlx::SqlitePool,
    chain_id: i64,
) -> Result<RpcEndpointRecord, ApplicationError> {
    let mut endpoints = sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
               failure_count, last_error, created_at, updated_at
        FROM eventlake_rpc_endpoints
        WHERE chain_id = $1 AND status != 'disabled'
        ORDER BY failure_count ASC, weight DESC, latency_ms ASC NULLS LAST, updated_at ASC
        "#,
    )
    .bind(chain_id)
    .fetch_all(pool)
    .await?;

    if endpoints.is_empty() {
        return Err(ApplicationError::NotFound(format!("RPC endpoint for chain {chain_id}")));
    }

    let now = Utc::now();
    for ep in &mut endpoints {
        let (until, remaining) = get_cooldown_info(&ep.id, now);
        ep.cooldown_until = until;
        ep.cooldown_remaining_seconds = remaining;
    }

    // 1. Healthy / enabled endpoints that are NOT in CD: dispatch proportionally via SWRR
    let available: Vec<_> = endpoints
        .iter()
        .filter(|ep| ep.cooldown_remaining_seconds.is_none() && (ep.status == "enabled" || ep.status == "healthy"))
        .cloned()
        .collect();

    if !available.is_empty() {
        return Ok(select_weighted_round_robin(&available));
    }

    // 2. Any non-disabled endpoint that is NOT in CD: dispatch proportionally via SWRR
    let non_cd: Vec<_> = endpoints
        .iter()
        .filter(|ep| ep.cooldown_remaining_seconds.is_none())
        .cloned()
        .collect();

    if !non_cd.is_empty() {
        return Ok(select_weighted_round_robin(&non_cd));
    }

    // 3. Fallback: all candidate endpoints are in CD. Choose the one with the shortest remaining CD
    endpoints.sort_by_key(|ep| ep.cooldown_remaining_seconds.unwrap_or(i64::MAX));
    let fallback = endpoints.remove(0);

    tracing::warn!(
        chain_id,
        endpoint_id = %fallback.id,
        url = %fallback.url,
        remaining_cd_secs = ?fallback.cooldown_remaining_seconds,
        "All RPC endpoints for chain are in cooldown; falling back to endpoint with shortest remaining CD"
    );

    Ok(fallback)
}

pub async fn mark_rpc_failure(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    error_message: &str,
) -> Result<(), ApplicationError> {
    let now = Utc::now();
    let (failures, cd_secs) = {
        let mut cache = ENDPOINT_RUNTIME_STATES
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = cache.entry(id).or_default();
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        let cd_secs = calculate_cooldown_seconds(entry.consecutive_failures);
        entry.cooldown_until = Some(now + chrono::Duration::seconds(cd_secs as i64));
        entry.last_error = Some(error_message.to_owned());
        (entry.consecutive_failures, cd_secs)
    };

    tracing::warn!(
        endpoint_id = %id,
        consecutive_failures = failures,
        cooldown_seconds = cd_secs,
        error = %error_message,
        "RPC endpoint failed; cooldown applied"
    );

    sqlx::query(
        r#"
        UPDATE eventlake_rpc_endpoints
        SET failure_count = failure_count + 1,
            last_error = $2,
            last_check_at = CURRENT_TIMESTAMP,
            status = CASE WHEN failure_count + 1 >= 3 THEN 'unhealthy' ELSE status END,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_rpc_success(
    pool: &sqlx::SqlitePool,
    id: Uuid,
) -> Result<(), ApplicationError> {
    let had_failures = {
        let mut cache = ENDPOINT_RUNTIME_STATES
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = cache.entry(id).or_default();
        let had_failures = entry.consecutive_failures > 0 || entry.cooldown_until.is_some();
        entry.consecutive_failures = 0;
        entry.cooldown_until = None;
        entry.last_error = None;
        entry.last_success_at = Some(Utc::now());
        had_failures
    };

    if had_failures {
        tracing::info!(endpoint_id = %id, "RPC endpoint recovered; cleared cooldown and reset healthy in DB");
        let _ = sqlx::query(
            r#"
            UPDATE eventlake_rpc_endpoints
            SET status = 'healthy',
                failure_count = 0,
                last_error = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND status != 'disabled'
            "#,
        )
        .bind(id)
        .execute(pool)
        .await;
    }

    Ok(())
}

async fn find_rpc_endpoint(
    pool: &sqlx::SqlitePool,
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
    pool: &sqlx::SqlitePool,
    id: Uuid,
    status: &str,
) -> Result<RpcEndpointRecord, ApplicationError> {
    sqlx::query_as::<_, RpcEndpointRecord>(
        r#"
        UPDATE eventlake_rpc_endpoints
        SET status = $2,
            updated_at = CURRENT_TIMESTAMP
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
    pool: &sqlx::SqlitePool,
    id: Uuid,
    check_result: Result<evm_rpc_client::RpcHealthCheck, ApplicationError>,
) -> Result<(), ApplicationError> {
    match check_result {
        Ok(check) => {
            let _ = mark_rpc_success(pool, id).await;
            sqlx::query(
                r#"
                UPDATE eventlake_rpc_endpoints
                SET status = 'healthy',
                    latency_ms = $2,
                    last_check_at = CURRENT_TIMESTAMP,
                    failure_count = 0,
                    last_error = NULL,
                    updated_at = CURRENT_TIMESTAMP
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
    if (cfg!(test) || std::env::var("RUST_TEST_THREADS").is_ok() || std::env::args().any(|arg| arg == "--test"))
        && std::env::var("EVENTLAKE_ENFORCE_SSRF_TEST").is_err()
    {
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

        let clean_host = host_str.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = clean_host.parse::<std::net::IpAddr>() {
            if is_private_ip(ip) {
                return Err(ApplicationError::BadRequest(
                    "private, loopback, or link-local RPC endpoint IP is not allowed".to_owned(),
                ));
            }
        } else {
            // It is a domain name. Resolve DNS to ensure none of the resolved IPs are private/loopback
            let port = parsed_url.port_or_known_default().unwrap_or(80);
            if let Ok(addresses) = std::net::ToSocketAddrs::to_socket_addrs(&(clean_host, port)) {
                for addr in addresses {
                    if is_private_ip(addr.ip()) {
                        return Err(ApplicationError::BadRequest(format!(
                            "RPC domain '{host_str}' resolves to private, loopback, or link-local IP {} which is not allowed",
                            addr.ip()
                        )));
                    }
                }
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
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // Unique Local Addresses (fc00::/7)
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local unicast (fe80::/10)
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped IPv6 addresses (::ffff:127.0.0.1)
                || match v6.to_ipv4_mapped() {
                    Some(v4) => is_private_ip(std::net::IpAddr::V4(v4)),
                    None => false,
                }
        }
    }
}

const SELECT_RPC_ENDPOINTS: &str = r#"
SELECT id, chain_id, url, status, weight, latency_ms, last_check_at,
       failure_count, last_error, created_at, updated_at
FROM eventlake_rpc_endpoints
ORDER BY chain_id, status, weight DESC, created_at
"#;

pub async fn seed_rpc_endpoints_from_file(
    pool: &sqlx::SqlitePool,
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
    pool: &sqlx::SqlitePool,
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

    #[test]
    fn test_validate_rpc_url_ssrf_blocking() {
        let localhost = reqwest::Url::parse("http://localhost:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&localhost).is_err());

        let internal_domain = reqwest::Url::parse("http://service.internal:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&internal_domain).is_err());

        let loopback_ip = reqwest::Url::parse("http://127.0.0.1:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&loopback_ip).is_err());

        let private_ip = reqwest::Url::parse("http://192.168.1.100:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&private_ip).is_err());

        let cloud_metadata = reqwest::Url::parse("http://169.254.169.254/latest").unwrap();
        assert!(validate_rpc_url_ssrf(&cloud_metadata).is_err());

        let ipv6_loopback = reqwest::Url::parse("http://[::1]:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&ipv6_loopback).is_err());

        let ipv6_mapped = reqwest::Url::parse("http://[::ffff:127.0.0.1]:8545").unwrap();
        assert!(validate_rpc_url_ssrf(&ipv6_mapped).is_err());
    }
}
