use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::{error::ApplicationError, validation::normalize_address},
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route(
            "/api/subscriptions",
            get(list_subscriptions).post(create_subscription),
        )
        .route(
            "/api/subscriptions/{id}",
            get(get_subscription).delete(delete_subscription),
        )
        .route(
            "/api/subscriptions/{id}/pause",
            axum::routing::post(pause_subscription),
        )
        .route(
            "/api/subscriptions/{id}/resume",
            axum::routing::post(resume_subscription),
        )
}

#[derive(Debug, Serialize, FromRow, Clone, ToSchema)]
pub struct SubscriptionRecord {
    pub id: Uuid,
    pub chain_id: i64,
    pub contract_address: String,
    pub abi_id: Option<Uuid>,
    pub start_block: i64,
    pub current_block: i64,
    pub target_block: Option<i64>,
    pub status: String,
    pub realtime_enabled: bool,
    pub active: bool,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSubscriptionRequest {
    pub chain_id: i64,
    pub contract_address: String,
    pub abi_id: Option<Uuid>,
    pub start_block: i64,
    pub realtime_enabled: Option<bool>,
}

async fn list_subscriptions(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<SubscriptionRecord>>>, ApplicationError> {
    let records = sqlx::query_as::<_, SubscriptionRecord>(SELECT_SUBSCRIPTIONS)
        .fetch_all(&state.pool)
        .await?;

    Ok(response::success(records))
}

async fn get_subscription(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    Ok(response::success(find_subscription(&state.pool, id).await?))
}

async fn create_subscription(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateSubscriptionRequest>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    principal.require_admin()?;

    let contract_address = normalize_address(&request.contract_address)?;

    if let Some(existing) =
        find_active_subscription_by_contract(&state.pool, request.chain_id, &contract_address)
            .await?
    {
        return Ok(response::success(existing));
    }

    let record = sqlx::query_as::<_, SubscriptionRecord>(
        r#"
        INSERT INTO subscriptions (
            id, chain_id, contract_address, abi_id, start_block, current_block, realtime_enabled
        )
        VALUES ($1, $2, $3, $4, $5, $5, $6)
        RETURNING id, chain_id, contract_address, abi_id, start_block, current_block,
                  target_block, status, realtime_enabled, active, error_message, created_at, updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(request.chain_id)
    .bind(contract_address)
    .bind(request.abi_id)
    .bind(request.start_block)
    .bind(request.realtime_enabled.unwrap_or(true))
    .fetch_one(&state.pool)
    .await?;

    upsert_contract_registry(
        &state.pool,
        record.chain_id,
        &record.contract_address,
        record.abi_id,
    )
    .await?;

    Ok(response::success(record))
}

async fn pause_subscription(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    principal.require_admin()?;
    Ok(response::success(
        update_status(&state.pool, id, "paused", true).await?,
    ))
}

async fn resume_subscription(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    principal.require_admin()?;
    Ok(response::success(
        update_status(&state.pool, id, "pending", true).await?,
    ))
}

async fn delete_subscription(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    principal.require_admin()?;
    Ok(response::success(
        update_status(&state.pool, id, "deleted", false).await?,
    ))
}

pub async fn runnable_subscriptions(
    pool: &sqlx::PgPool,
    limit: i64,
) -> Result<Vec<SubscriptionRecord>, ApplicationError> {
    let records = sqlx::query_as::<_, SubscriptionRecord>(
        r#"
        SELECT id, chain_id, contract_address, abi_id, start_block, current_block,
               target_block, status, realtime_enabled, active, error_message, created_at, updated_at
        FROM subscriptions
        WHERE active = true
          AND status IN ('pending', 'historical_syncing', 'realtime_syncing', 'error')
        ORDER BY updated_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(records)
}

pub async fn update_checkpoint(
    pool: &sqlx::PgPool,
    id: Uuid,
    next_block: i64,
    target_block: Option<i64>,
    status: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE subscriptions
        SET current_block = $2,
            target_block = $3,
            status = $4,
            error_message = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(next_block)
    .bind(target_block)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_subscription_error(
    pool: &sqlx::PgPool,
    id: Uuid,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE subscriptions
        SET status = 'error',
            error_message = $2,
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

async fn find_subscription(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<SubscriptionRecord, ApplicationError> {
    sqlx::query_as::<_, SubscriptionRecord>(
        r#"
        SELECT id, chain_id, contract_address, abi_id, start_block, current_block,
               target_block, status, realtime_enabled, active, error_message, created_at, updated_at
        FROM subscriptions
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("subscription {id}")))
}

async fn find_active_subscription_by_contract(
    pool: &sqlx::PgPool,
    chain_id: i64,
    contract_address: &str,
) -> Result<Option<SubscriptionRecord>, ApplicationError> {
    let record = sqlx::query_as::<_, SubscriptionRecord>(
        r#"
        SELECT id, chain_id, contract_address, abi_id, start_block, current_block,
               target_block, status, realtime_enabled, active, error_message, created_at, updated_at
        FROM subscriptions
        WHERE chain_id = $1 AND contract_address = $2 AND active = true
        "#,
    )
    .bind(chain_id)
    .bind(contract_address)
    .fetch_optional(pool)
    .await?;

    Ok(record)
}

async fn update_status(
    pool: &sqlx::PgPool,
    id: Uuid,
    status: &str,
    active: bool,
) -> Result<SubscriptionRecord, ApplicationError> {
    sqlx::query_as::<_, SubscriptionRecord>(
        r#"
        UPDATE subscriptions
        SET status = $2,
            active = $3,
            updated_at = now()
        WHERE id = $1
        RETURNING id, chain_id, contract_address, abi_id, start_block, current_block,
                  target_block, status, realtime_enabled, active, error_message, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(active)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("subscription {id}")))
}

async fn upsert_contract_registry(
    pool: &sqlx::PgPool,
    chain_id: i64,
    contract_address: &str,
    abi_id: Option<Uuid>,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        INSERT INTO contract_registry (id, chain_id, contract_address, abi_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (chain_id, contract_address) DO UPDATE
        SET abi_id = EXCLUDED.abi_id,
            updated_at = now()
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(chain_id)
    .bind(contract_address)
    .bind(abi_id)
    .execute(pool)
    .await?;

    Ok(())
}

const SELECT_SUBSCRIPTIONS: &str = r#"
SELECT id, chain_id, contract_address, abi_id, start_block, current_block,
       target_block, status, realtime_enabled, active, error_message, created_at, updated_at
FROM subscriptions
ORDER BY created_at DESC
"#;
