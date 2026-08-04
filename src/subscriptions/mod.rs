use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
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

#[derive(OpenApi)]
#[openapi(
    paths(
        list_subscriptions,
        get_subscription,
        create_subscription,
        pause_subscription,
        resume_subscription,
        delete_subscription
    ),
    components(schemas(SubscriptionRecord, CreateSubscriptionRequest))
)]
struct SubscriptionsApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    SubscriptionsApiDocumentation::openapi()
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
    pub min_block_window: i64,
    pub max_block_window: i64,
    pub current_block_window: i64,
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
    pub min_block_window: Option<i64>,
    pub max_block_window: Option<i64>,
}

/// The full `SubscriptionRecord` column list, kept in one place so the many SELECT and
/// RETURNING clauses below cannot drift out of sync with each other or the struct.
const SUBSCRIPTION_COLUMNS: &str = "id, chain_id, contract_address, abi_id, start_block, \
    current_block, target_block, min_block_window, max_block_window, current_block_window, \
    status, realtime_enabled, active, error_message, created_at, updated_at";

#[utoipa::path(
    get,
    path = "/api/subscriptions",
    tag = "subscriptions",
    responses((status = 200, description = "Subscriptions", body = ApiResponse<Vec<SubscriptionRecord>>))
)]
async fn list_subscriptions(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<SubscriptionRecord>>>, ApplicationError> {
    let query = format!(
        "SELECT {SUBSCRIPTION_COLUMNS} FROM eventlake_subscriptions ORDER BY created_at DESC"
    );
    let records = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(query))
        .fetch_all(&state.pool)
        .await?;

    Ok(response::success(records))
}

#[utoipa::path(
    get,
    path = "/api/subscriptions/{id}",
    tag = "subscriptions",
    params(("id" = uuid::Uuid, Path, description = "Subscription id")),
    responses(
        (status = 200, description = "Subscription", body = ApiResponse<SubscriptionRecord>),
        (status = 404, description = "Subscription not found")
    )
)]
async fn get_subscription(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    Ok(response::success(find_subscription(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/subscriptions",
    tag = "subscriptions",
    request_body = CreateSubscriptionRequest,
    responses(
        (status = 200, description = "Subscription created or existing active subscription returned", body = ApiResponse<SubscriptionRecord>),
        (status = 400, description = "Invalid subscription request"),
        (status = 409, description = "Subscription conflict")
    )
)]
async fn create_subscription(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateSubscriptionRequest>,
) -> Result<Json<ApiResponse<SubscriptionRecord>>, ApplicationError> {
    principal.require_admin()?;

    validate_subscription_request(&request)?;
    let contract_address = normalize_address(&request.contract_address)?;

    let collection_policy = chains::get_collection_policy(&state.pool, request.chain_id).await?;
    let min_block_window = request
        .min_block_window
        .unwrap_or(collection_policy.default_min_block_window);
    let max_block_window = request
        .max_block_window
        .unwrap_or(collection_policy.default_max_block_window);
    validate_block_windows(min_block_window, max_block_window)?;

    let insert_query = format!(
        r#"
        INSERT INTO eventlake_subscriptions (
            id, chain_id, contract_address, abi_id, start_block, current_block,
            min_block_window, max_block_window, current_block_window, realtime_enabled
        )
        VALUES ($1, $2, $3, $4, $5, $5, $6, $7, $7, $8)
        ON CONFLICT (chain_id, contract_address) WHERE active = true DO NOTHING
        RETURNING {SUBSCRIPTION_COLUMNS}
        "#
    );
    let inserted = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(insert_query))
        .bind(Uuid::new_v4())
        .bind(request.chain_id)
        .bind(&contract_address)
        .bind(request.abi_id)
        .bind(request.start_block)
        .bind(min_block_window)
        .bind(max_block_window)
        .bind(request.realtime_enabled.unwrap_or(true))
        .fetch_optional(&state.pool)
        .await?;

    let record = match inserted {
        Some(record) => record,
        None => {
            find_active_subscription_by_contract(&state.pool, request.chain_id, &contract_address)
                .await?
                .ok_or_else(|| {
                    ApplicationError::Conflict(
                        "active subscription conflict was detected but existing row was not found"
                            .to_owned(),
                    )
                })?
        }
    };

    upsert_contract_registry(
        &state.pool,
        record.chain_id,
        &record.contract_address,
        record.abi_id,
    )
    .await?;

    Ok(response::success(record))
}

#[utoipa::path(
    post,
    path = "/api/subscriptions/{id}/pause",
    tag = "subscriptions",
    params(("id" = uuid::Uuid, Path, description = "Subscription id")),
    responses((status = 200, description = "Subscription paused", body = ApiResponse<SubscriptionRecord>))
)]
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

#[utoipa::path(
    post,
    path = "/api/subscriptions/{id}/resume",
    tag = "subscriptions",
    params(("id" = uuid::Uuid, Path, description = "Subscription id")),
    responses((status = 200, description = "Subscription resumed", body = ApiResponse<SubscriptionRecord>))
)]
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

#[utoipa::path(
    delete,
    path = "/api/subscriptions/{id}",
    tag = "subscriptions",
    params(("id" = uuid::Uuid, Path, description = "Subscription id")),
    responses((status = 200, description = "Subscription deleted", body = ApiResponse<SubscriptionRecord>))
)]
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
    let query = format!(
        r#"
        SELECT {SUBSCRIPTION_COLUMNS}
        FROM eventlake_subscriptions
        WHERE active = true
          AND status IN (
              'pending', 'historical_syncing', 'realtime_syncing', 'error',
              'clickhouse_reorg_retrying'
          )
        ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, updated_at ASC
        LIMIT $1
        "#
    );
    let records = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(query))
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
    current_block_window: i64,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET current_block = $2,
            target_block = $3,
            status = $4,
            current_block_window = $5,
            error_message = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(next_block)
    .bind(target_block)
    .bind(status)
    .bind(current_block_window)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_collection_window_after_retryable_error(
    pool: &sqlx::PgPool,
    id: Uuid,
    current_block_window: i64,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET current_block_window = $2,
            status = CASE WHEN status = 'pending' THEN 'historical_syncing' ELSE status END,
            error_message = $3,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(current_block_window)
    .bind(error_message)
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
        UPDATE eventlake_subscriptions
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

/// A ClickHouse decoded-event write has not completed. Collection pauses for this
/// subscription while decoder retries the queued raw log; it resumes automatically
/// once no ClickHouse retry entries remain.
pub async fn mark_clickhouse_write_retrying(
    pool: &sqlx::PgPool,
    id: Uuid,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET status = 'clickhouse_write_retrying',
            error_message = $2,
            updated_at = now()
        WHERE id = $1 AND active = true
        "#,
    )
    .bind(id)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

/// Restores collection after every pending ClickHouse write for this subscription has
/// succeeded. Non-ClickHouse decode errors retain their own retry policy and do not
/// keep an otherwise healthy subscription blocked forever.
pub async fn resume_after_clickhouse_writes(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions s
        SET status = 'pending',
            error_message = NULL,
            updated_at = now()
        WHERE s.id = $1
          AND s.active = true
          AND s.status = 'clickhouse_write_retrying'
          AND NOT EXISTS (
              SELECT 1
              FROM eventlake_decode_queue q
              WHERE q.subscription_id = s.id
                AND q.status IN ('pending', 'clickhouse_retrying')
          )
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// A reorg needs ClickHouse tombstones before any affected subscription can collect
/// the canonical fork again. The collector retries this state on every worker tick.
pub async fn mark_chain_clickhouse_reorg_retrying(
    pool: &sqlx::PgPool,
    chain_id: i64,
    from_block: i64,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET status = 'clickhouse_reorg_retrying',
            error_message = $3,
            updated_at = now()
        WHERE chain_id = $1
          AND active = true
          AND current_block >= $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn resume_after_clickhouse_reorg(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET status = 'pending',
            error_message = NULL,
            updated_at = now()
        WHERE id = $1 AND active = true AND status = 'clickhouse_reorg_retrying'
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn find_subscription(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<SubscriptionRecord, ApplicationError> {
    let query = format!("SELECT {SUBSCRIPTION_COLUMNS} FROM eventlake_subscriptions WHERE id = $1");
    sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(query))
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
    let query = format!(
        r#"
        SELECT {SUBSCRIPTION_COLUMNS}
        FROM eventlake_subscriptions
        WHERE chain_id = $1 AND contract_address = $2 AND active = true
        "#
    );
    let record = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(query))
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
    let query = format!(
        r#"
        UPDATE eventlake_subscriptions
        SET status = $2,
            active = $3,
            error_message = CASE WHEN $2 = 'pending' THEN NULL ELSE error_message END,
            updated_at = now()
        WHERE id = $1
        RETURNING {SUBSCRIPTION_COLUMNS}
        "#
    );
    sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(query))
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
        INSERT INTO eventlake_contract_registry (id, chain_id, contract_address, abi_id)
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

fn validate_subscription_request(
    request: &CreateSubscriptionRequest,
) -> Result<(), ApplicationError> {
    if request.chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be greater than 0".to_owned(),
        ));
    }

    if request.start_block < 0 {
        return Err(ApplicationError::BadRequest(
            "start_block must be greater than or equal to 0".to_owned(),
        ));
    }

    Ok(())
}

fn validate_block_windows(
    min_block_window: i64,
    max_block_window: i64,
) -> Result<(), ApplicationError> {
    if min_block_window < 1 {
        return Err(ApplicationError::BadRequest(
            "min_block_window must be at least 1".to_owned(),
        ));
    }

    if max_block_window < min_block_window {
        return Err(ApplicationError::BadRequest(
            "max_block_window must be greater than or equal to min_block_window".to_owned(),
        ));
    }

    Ok(())
}
