use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

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

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ChainRecord {
    pub chain_id: i64,
    pub name: String,
    pub native_token_symbol: String,
    pub status: String,
    pub safe_confirmation_depth: i64,
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
    pub default_max_block_window: Option<i64>,
    pub rpc_notes: Option<String>,
}

async fn list_chains(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<ChainRecord>>>, ApplicationError> {
    let chains = sqlx::query_as::<_, ChainRecord>(
        r#"
        SELECT chain_id, name, native_token_symbol, status, safe_confirmation_depth,
               default_max_block_window, rpc_notes, created_at, updated_at
        FROM chains
        ORDER BY chain_id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(response::success(chains))
}

async fn get_chain(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<ChainRecord>>, ApplicationError> {
    let chain = sqlx::query_as::<_, ChainRecord>(
        r#"
        SELECT chain_id, name, native_token_symbol, status, safe_confirmation_depth,
               default_max_block_window, rpc_notes, created_at, updated_at
        FROM chains
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("chain {chain_id}")))?;

    Ok(response::success(chain))
}

async fn create_chain(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateChainRequest>,
) -> Result<Json<ApiResponse<ChainRecord>>, ApplicationError> {
    principal.require_admin()?;

    let chain = sqlx::query_as::<_, ChainRecord>(
        r#"
        INSERT INTO chains (
            chain_id, name, native_token_symbol, safe_confirmation_depth,
            default_max_block_window, rpc_notes
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (chain_id) DO UPDATE
        SET name = EXCLUDED.name,
            native_token_symbol = EXCLUDED.native_token_symbol,
            safe_confirmation_depth = EXCLUDED.safe_confirmation_depth,
            default_max_block_window = EXCLUDED.default_max_block_window,
            rpc_notes = EXCLUDED.rpc_notes,
            updated_at = now()
        RETURNING chain_id, name, native_token_symbol, status, safe_confirmation_depth,
                  default_max_block_window, rpc_notes, created_at, updated_at
        "#,
    )
    .bind(request.chain_id)
    .bind(request.name)
    .bind(request.native_token_symbol)
    .bind(request.safe_confirmation_depth.unwrap_or(12))
    .bind(request.default_max_block_window.unwrap_or(1000))
    .bind(request.rpc_notes)
    .fetch_one(&state.pool)
    .await?;

    Ok(response::success(chain))
}

pub async fn get_collection_policy(
    pool: &sqlx::PgPool,
    chain_id: i64,
) -> Result<(i64, i64), ApplicationError> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        "SELECT safe_confirmation_depth, default_max_block_window FROM chains WHERE chain_id = $1",
    )
    .bind(chain_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("chain {chain_id}")))?;

    Ok(row)
}
