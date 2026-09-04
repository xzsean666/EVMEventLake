use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder};
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
            "/api/subscriptions/batch",
            post(create_contract_subscriptions_batch),
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
        create_contract_subscriptions_batch,
        pause_subscription,
        resume_subscription,
        delete_subscription
    ),
    components(schemas(
        SubscriptionRecord,
        CreateSubscriptionRequest,
        CreateContractSubscriptionsRequest,
        CollectionScope
    ))
)]
struct SubscriptionsApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    SubscriptionsApiDocumentation::openapi()
}

#[derive(Debug, Serialize, FromRow, Clone, ToSchema)]
pub struct SubscriptionRecord {
    pub id: Uuid,
    pub chain_id: i64,
    /// `None` identifies an all-events subscription. Contract-scoped subscriptions
    /// always have a normalized address.
    pub contract_address: Option<String>,
    pub collection_scope: String,
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
    /// Omit this only with `collection_scope: "all_events"`.
    pub contract_address: Option<String>,
    /// `contract` is backward-compatible and requires `contract_address`.
    /// `all_events` issues `eth_getLogs` without an address filter.
    pub collection_scope: Option<CollectionScope>,
    /// Retained only for request compatibility. Raw-event-lake collection does not
    /// decode logs, so this value is not used by the collector.
    pub abi_id: Option<Uuid>,
    pub start_block: i64,
    pub realtime_enabled: Option<bool>,
    pub min_block_window: Option<i64>,
    pub max_block_window: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateContractSubscriptionsRequest {
    pub chain_id: i64,
    /// Contract addresses are normalized and de-duplicated before subscriptions are created.
    pub contract_addresses: Vec<String>,
    /// Optional because raw collection does not decode logs.
    pub abi_id: Option<Uuid>,
    pub start_block: i64,
    pub realtime_enabled: Option<bool>,
    pub min_block_window: Option<i64>,
    pub max_block_window: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CollectionScope {
    Contract,
    AllEvents,
}

impl CollectionScope {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::AllEvents => "all_events",
        }
    }
}

/// The full `SubscriptionRecord` column list, kept in one place so the many SELECT and
/// RETURNING clauses below cannot drift out of sync with each other or the struct.
const SUBSCRIPTION_COLUMNS: &str = "id, chain_id, contract_address, collection_scope, abi_id, start_block, \
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
    let collection_scope = request
        .collection_scope
        .as_ref()
        .map(CollectionScope::as_str)
        .unwrap_or("contract");
    let contract_address = match collection_scope {
        "contract" => Some(normalize_address(
            request.contract_address.as_deref().ok_or_else(|| {
                ApplicationError::BadRequest(
                    "contract_address is required when collection_scope is contract".to_owned(),
                )
            })?,
        )?),
        "all_events" => {
            if request.contract_address.is_some() {
                return Err(ApplicationError::BadRequest(
                    "contract_address must be omitted when collection_scope is all_events"
                        .to_owned(),
                ));
            }
            if !state.configuration.clickhouse.enabled {
                return Err(ApplicationError::BadRequest(
                    "all_events collection requires EVENTLAKE_CLICKHOUSE_ENABLED=true".to_owned(),
                ));
            }
            None
        }
        _ => unreachable!("CollectionScope only exposes known values"),
    };

    let collection_policy = chains::get_collection_policy(&state.pool, request.chain_id).await?;
    let min_block_window = request
        .min_block_window
        .unwrap_or(collection_policy.default_min_block_window);
    let max_block_window = request
        .max_block_window
        .unwrap_or(collection_policy.default_max_block_window);
    validate_block_windows(min_block_window, max_block_window)?;

    // Serialize subscription changes per chain. This closes the race where one request
    // creates `all_events` while another creates a contract scope on the same chain.
    let mut transaction = state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(request.chain_id)
        .execute(&mut *transaction)
        .await?;

    let all_events_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM eventlake_subscriptions WHERE chain_id = $1 AND collection_scope = 'all_events' AND active = true)",
    )
    .bind(request.chain_id)
    .fetch_one(&mut *transaction)
    .await?;

    if collection_scope == "contract" && all_events_active {
        return Err(ApplicationError::Conflict(
            "an active all_events subscription already collects this chain".to_owned(),
        ));
    }

    if collection_scope == "all_events" {
        let existing_all_events =
            sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(format!(
                "SELECT {SUBSCRIPTION_COLUMNS} FROM eventlake_subscriptions \
                 WHERE chain_id = $1 AND collection_scope = 'all_events' AND active = true"
            )))
            .bind(request.chain_id)
            .fetch_optional(&mut *transaction)
            .await?;

        if let Some(record) = existing_all_events {
            transaction.commit().await?;
            return Ok(response::success(record));
        }

        let contract_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM eventlake_subscriptions WHERE chain_id = $1 AND collection_scope = 'contract' AND active = true)",
        )
        .bind(request.chain_id)
        .fetch_one(&mut *transaction)
        .await?;
        if contract_active {
            return Err(ApplicationError::Conflict(
                "active contract subscriptions already collect this chain; pause or delete them before enabling all_events"
                    .to_owned(),
            ));
        }
    }

    let insert_query = format!(
        r#"
        INSERT INTO eventlake_subscriptions (
            id, chain_id, contract_address, collection_scope, abi_id, start_block, current_block,
            min_block_window, max_block_window, current_block_window, realtime_enabled
        )
        VALUES ($1, $2, $3, $4, $5, $6, $6, $7, $8, $8, $9)
        ON CONFLICT DO NOTHING
        RETURNING {SUBSCRIPTION_COLUMNS}
        "#
    );
    let inserted = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(insert_query))
        .bind(Uuid::new_v4())
        .bind(request.chain_id)
        .bind(&contract_address)
        .bind(collection_scope)
        .bind(request.abi_id)
        .bind(request.start_block)
        .bind(min_block_window)
        .bind(max_block_window)
        .bind(request.realtime_enabled.unwrap_or(true))
        .fetch_optional(&mut *transaction)
        .await?;

    let record = match inserted {
        Some(record) => record,
        None => sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(format!(
            "SELECT {SUBSCRIPTION_COLUMNS} FROM eventlake_subscriptions \
                 WHERE chain_id = $1 AND collection_scope = $2 \
                   AND contract_address IS NOT DISTINCT FROM $3 AND active = true"
        )))
        .bind(request.chain_id)
        .bind(collection_scope)
        .bind(contract_address.as_deref())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| {
            ApplicationError::Conflict(
                "active subscription conflict was detected but existing row was not found"
                    .to_owned(),
            )
        })?,
    };

    transaction.commit().await?;

    if let Some(contract_address) = record.contract_address.as_deref() {
        upsert_contract_registry(
            &state.pool,
            record.chain_id,
            contract_address,
            record.abi_id,
        )
        .await?;
    }

    Ok(response::success(record))
}

#[utoipa::path(
    post,
    path = "/api/subscriptions/batch",
    tag = "subscriptions",
    request_body = CreateContractSubscriptionsRequest,
    responses(
        (status = 200, description = "Contract subscriptions created or existing active subscriptions returned", body = ApiResponse<Vec<SubscriptionRecord>>),
        (status = 400, description = "Invalid batch subscription request"),
        (status = 409, description = "Subscription scope conflict")
    )
)]
async fn create_contract_subscriptions_batch(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<CreateContractSubscriptionsRequest>,
) -> Result<Json<ApiResponse<Vec<SubscriptionRecord>>>, ApplicationError> {
    principal.require_admin()?;

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
    if request.contract_addresses.is_empty() {
        return Err(ApplicationError::BadRequest(
            "contract_addresses must contain at least one address".to_owned(),
        ));
    }
    if request.contract_addresses.len() > 1_000 {
        return Err(ApplicationError::BadRequest(
            "contract_addresses cannot contain more than 1000 addresses".to_owned(),
        ));
    }

    // Normalize and de-duplicate the input before touching the database. This makes retries
    // safe and ensures one request cannot create duplicate work for the same contract.
    let mut addresses = Vec::with_capacity(request.contract_addresses.len());
    let mut seen = HashSet::with_capacity(request.contract_addresses.len());
    for address in &request.contract_addresses {
        let normalized = normalize_address(address)?;
        if seen.insert(normalized.clone()) {
            addresses.push(normalized);
        }
    }

    let collection_policy = chains::get_collection_policy(&state.pool, request.chain_id).await?;
    let min_block_window = request
        .min_block_window
        .unwrap_or(collection_policy.default_min_block_window);
    let max_block_window = request
        .max_block_window
        .unwrap_or(collection_policy.default_max_block_window);
    validate_block_windows(min_block_window, max_block_window)?;

    // Single transaction with a single advisory lock per chain
    let mut transaction = state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(request.chain_id)
        .execute(&mut *transaction)
        .await?;

    let all_events_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM eventlake_subscriptions WHERE chain_id = $1 AND collection_scope = 'all_events' AND active = true)",
    )
    .bind(request.chain_id)
    .fetch_one(&mut *transaction)
    .await?;

    if all_events_active {
        return Err(ApplicationError::Conflict(
            "an active all_events subscription already collects this chain".to_owned(),
        ));
    }

    let realtime = request.realtime_enabled.unwrap_or(true);
    for chunk in addresses.chunks(200) {
        let mut builder = QueryBuilder::new(
            r#"
            INSERT INTO eventlake_subscriptions (
                id, chain_id, contract_address, collection_scope, abi_id, start_block, current_block,
                min_block_window, max_block_window, current_block_window, realtime_enabled
            )
            "#,
        );
        builder.push_values(chunk, |mut row, addr| {
            row.push_bind(Uuid::new_v4())
                .push_bind(request.chain_id)
                .push_bind(addr)
                .push_bind("contract")
                .push_bind(request.abi_id)
                .push_bind(request.start_block)
                .push_bind(request.start_block)
                .push_bind(min_block_window)
                .push_bind(max_block_window)
                .push_bind(max_block_window)
                .push_bind(realtime);
        });
        builder.push(" ON CONFLICT DO NOTHING");
        builder.build().execute(&mut *transaction).await?;
    }

    for chunk in addresses.chunks(200) {
        let mut builder = QueryBuilder::new(
            r#"
            INSERT INTO eventlake_contract_registry (id, chain_id, contract_address, abi_id)
            "#,
        );
        builder.push_values(chunk, |mut row, addr| {
            row.push_bind(Uuid::new_v4())
                .push_bind(request.chain_id)
                .push_bind(addr)
                .push_bind(request.abi_id);
        });
        builder.push(
            r#"
            ON CONFLICT (chain_id, contract_address) DO UPDATE
            SET abi_id = EXCLUDED.abi_id,
                updated_at = now()
            "#,
        );
        builder.build().execute(&mut *transaction).await?;
    }

    let select_query = format!(
        "SELECT {SUBSCRIPTION_COLUMNS} FROM eventlake_subscriptions \
         WHERE chain_id = $1 AND collection_scope = 'contract' \
           AND contract_address = ANY($2) AND active = true"
    );
    let records = sqlx::query_as::<_, SubscriptionRecord>(sqlx::AssertSqlSafe(select_query))
        .bind(request.chain_id)
        .bind(&addresses)
        .fetch_all(&mut *transaction)
        .await?;

    transaction.commit().await?;

    let mut record_map: HashMap<String, SubscriptionRecord> = HashMap::new();
    for r in records {
        if let Some(ref ca) = r.contract_address {
            record_map.insert(ca.clone(), r);
        }
    }
    let ordered_records: Vec<SubscriptionRecord> = addresses
        .into_iter()
        .filter_map(|addr| record_map.remove(&addr))
        .collect();

    Ok(response::success(ordered_records))
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

#[derive(Debug, Clone)]
pub struct CheckpointBatchUpdate {
    pub id: Uuid,
    pub next_block: i64,
    pub target_block: Option<i64>,
    pub status: String,
    pub current_block_window: i64,
}

pub async fn update_checkpoints_batch(
    pool: &sqlx::PgPool,
    updates: &[CheckpointBatchUpdate],
) -> Result<(), ApplicationError> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for update in updates {
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
        .bind(update.id)
        .bind(update.next_block)
        .bind(update.target_block)
        .bind(&update.status)
        .bind(update.current_block_window)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
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

pub async fn update_collection_windows_after_retryable_error(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
    current_block_window: i64,
    error_message: &str,
) -> Result<(), ApplicationError> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for id in ids {
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
        .bind(*id)
        .bind(current_block_window)
        .bind(error_message)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
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
