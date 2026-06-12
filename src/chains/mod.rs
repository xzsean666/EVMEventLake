use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::error::ApplicationError,
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route("/api/chains", get(list_chains).post(create_chain))
        .route("/api/chains/{chain_id}", get(get_chain))
}

#[derive(OpenApi)]
#[openapi(
    paths(list_chains, get_chain, create_chain),
    components(schemas(ChainRecord, CreateChainRequest))
)]
struct ChainsApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ChainsApiDocumentation::openapi()
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ChainRecord {
    pub chain_id: i64,
    pub name: String,
    pub native_token_symbol: String,
    pub status: String,
    pub safe_confirmation_depth: i64,
    pub default_min_block_window: i64,
    pub default_max_block_window: i64,
    pub rpc_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateChainRequest {
    pub chain_id: i64,
    pub name: String,
    pub native_token_symbol: String,
    pub safe_confirmation_depth: Option<i64>,
    pub default_min_block_window: Option<i64>,
    pub default_max_block_window: Option<i64>,
    pub rpc_notes: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CollectionPolicy {
    pub safe_confirmation_depth: i64,
    pub default_min_block_window: i64,
    pub default_max_block_window: i64,
}

#[utoipa::path(
    get,
    path = "/api/chains",
    tag = "chains",
    responses((status = 200, description = "Chains", body = ApiResponse<Vec<ChainRecord>>))
)]
async fn list_chains(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<ChainRecord>>>, ApplicationError> {
    let chains = sqlx::query_as::<_, ChainRecord>(
        r#"
        SELECT chain_id, name, native_token_symbol, status, safe_confirmation_depth,
               default_min_block_window, default_max_block_window, rpc_notes, created_at, updated_at
        FROM eventlake_chains
        ORDER BY chain_id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(response::success(chains))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}",
    tag = "chains",
    params(("chain_id" = i64, Path, description = "EVM chain id")),
    responses(
        (status = 200, description = "Chain", body = ApiResponse<ChainRecord>),
        (status = 404, description = "Chain not found")
    )
)]
async fn get_chain(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<ChainRecord>>, ApplicationError> {
    let chain = sqlx::query_as::<_, ChainRecord>(
        r#"
        SELECT chain_id, name, native_token_symbol, status, safe_confirmation_depth,
               default_min_block_window, default_max_block_window, rpc_notes, created_at, updated_at
        FROM eventlake_chains
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("chain {chain_id}")))?;

    Ok(response::success(chain))
}

#[utoipa::path(
    post,
    path = "/api/chains",
    tag = "chains",
    request_body = CreateChainRequest,
    responses(
        (status = 200, description = "Chain created or updated", body = ApiResponse<ChainRecord>),
        (status = 400, description = "Invalid chain request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
async fn create_chain(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateChainRequest>,
) -> Result<Json<ApiResponse<ChainRecord>>, ApplicationError> {
    principal.require_admin()?;

    let safe_confirmation_depth = request.safe_confirmation_depth.unwrap_or(12);
    let default_min_block_window = request.default_min_block_window.unwrap_or(1);
    let default_max_block_window = request.default_max_block_window.unwrap_or(1000);
    validate_chain_request(
        &request,
        safe_confirmation_depth,
        default_min_block_window,
        default_max_block_window,
    )?;

    let chain = sqlx::query_as::<_, ChainRecord>(
        r#"
        INSERT INTO eventlake_chains (
            chain_id, name, native_token_symbol, safe_confirmation_depth,
            default_min_block_window, default_max_block_window, rpc_notes
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (chain_id) DO UPDATE
        SET name = EXCLUDED.name,
            native_token_symbol = EXCLUDED.native_token_symbol,
            safe_confirmation_depth = EXCLUDED.safe_confirmation_depth,
            default_min_block_window = EXCLUDED.default_min_block_window,
            default_max_block_window = EXCLUDED.default_max_block_window,
            rpc_notes = EXCLUDED.rpc_notes,
            updated_at = now()
        RETURNING chain_id, name, native_token_symbol, status, safe_confirmation_depth,
                  default_min_block_window, default_max_block_window, rpc_notes, created_at, updated_at
        "#,
    )
    .bind(request.chain_id)
    .bind(request.name)
    .bind(request.native_token_symbol)
    .bind(safe_confirmation_depth)
    .bind(default_min_block_window)
    .bind(default_max_block_window)
    .bind(request.rpc_notes)
    .fetch_one(&state.pool)
    .await?;

    Ok(response::success(chain))
}

pub async fn get_collection_policy(
    pool: &sqlx::PgPool,
    chain_id: i64,
) -> Result<CollectionPolicy, ApplicationError> {
    let row = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT safe_confirmation_depth, default_min_block_window, default_max_block_window
        FROM eventlake_chains
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("chain {chain_id}")))?;

    Ok(CollectionPolicy {
        safe_confirmation_depth: row.0,
        default_min_block_window: row.1,
        default_max_block_window: row.2,
    })
}

fn validate_chain_request(
    request: &CreateChainRequest,
    safe_confirmation_depth: i64,
    default_min_block_window: i64,
    default_max_block_window: i64,
) -> Result<(), ApplicationError> {
    if request.chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be greater than 0".to_owned(),
        ));
    }

    if request.name.trim().is_empty() {
        return Err(ApplicationError::BadRequest(
            "chain name must not be empty".to_owned(),
        ));
    }

    if request.native_token_symbol.trim().is_empty() {
        return Err(ApplicationError::BadRequest(
            "native_token_symbol must not be empty".to_owned(),
        ));
    }

    if safe_confirmation_depth < 0 {
        return Err(ApplicationError::BadRequest(
            "safe_confirmation_depth must be greater than or equal to 0".to_owned(),
        ));
    }

    validate_default_block_windows(default_min_block_window, default_max_block_window)
}

fn validate_default_block_windows(
    default_min_block_window: i64,
    default_max_block_window: i64,
) -> Result<(), ApplicationError> {
    if default_min_block_window < 1 {
        return Err(ApplicationError::BadRequest(
            "default_min_block_window must be at least 1".to_owned(),
        ));
    }

    if default_max_block_window < default_min_block_window {
        return Err(ApplicationError::BadRequest(
            "default_max_block_window must be greater than or equal to default_min_block_window"
                .to_owned(),
        ));
    }

    Ok(())
}
