use alloy_json_abi::{Event, JsonAbi};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::{error::ApplicationError, validation::normalize_topic},
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route("/api/abis", get(list_abis).post(upload_abi))
        .route("/api/abis/{id}", get(get_abi).delete(delete_abi))
        .route("/api/events", get(list_events))
}

#[derive(Debug, Serialize, FromRow, Clone, ToSchema)]
pub struct AbiVersionRecord {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
    pub abi_json: Value,
    pub status: String,
    pub event_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, Clone, ToSchema)]
pub struct EventRegistryRecord {
    pub id: Uuid,
    pub abi_id: Uuid,
    pub event_name: String,
    pub signature: String,
    pub topic0: String,
    pub inputs: Value,
    pub indexed_inputs: Value,
    pub anonymous: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadAbiRequest {
    pub name: String,
    pub abi_json: Value,
}

async fn list_abis(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<AbiVersionRecord>>>, ApplicationError> {
    let records = sqlx::query_as::<_, AbiVersionRecord>(
        r#"
        SELECT id, name, version, abi_json, status, event_count, created_at
        FROM abi_versions
        ORDER BY name, version DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(response::success(records))
}

async fn get_abi(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<AbiVersionRecord>>, ApplicationError> {
    let record = find_abi(&state.pool, id).await?;
    Ok(response::success(record))
}

async fn upload_abi(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<UploadAbiRequest>,
) -> Result<Json<ApiResponse<AbiVersionRecord>>, ApplicationError> {
    principal.require_admin()?;

    let abi_json_text = serde_json::to_string(&request.abi_json)
        .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;
    let parsed_abi = JsonAbi::from_json_str(&abi_json_text)
        .map_err(|error| ApplicationError::BadRequest(format!("invalid ABI JSON: {error}")))?;

    let next_version = sqlx::query_as::<_, (i32,)>(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM abi_versions WHERE name = $1",
    )
    .bind(&request.name)
    .fetch_one(&state.pool)
    .await?
    .0;

    let abi_id = Uuid::new_v4();
    let event_count = parsed_abi.events.values().map(Vec::len).sum::<usize>() as i32;

    let record = sqlx::query_as::<_, AbiVersionRecord>(
        r#"
        INSERT INTO abi_versions (id, name, version, abi_json, event_count)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, version, abi_json, status, event_count, created_at
        "#,
    )
    .bind(abi_id)
    .bind(&request.name)
    .bind(next_version)
    .bind(&request.abi_json)
    .bind(event_count)
    .fetch_one(&state.pool)
    .await?;

    for event in parsed_abi.events.values().flatten() {
        insert_event_registry_record(&state.pool, abi_id, event).await?;
    }

    Ok(response::success(record))
}

async fn delete_abi(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<AbiVersionRecord>>, ApplicationError> {
    principal.require_admin()?;

    let record = sqlx::query_as::<_, AbiVersionRecord>(
        r#"
        UPDATE abi_versions
        SET status = 'deleted'
        WHERE id = $1
        RETURNING id, name, version, abi_json, status, event_count, created_at
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("abi {id}")))?;

    Ok(response::success(record))
}

async fn list_events(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
) -> Result<Json<ApiResponse<Vec<EventRegistryRecord>>>, ApplicationError> {
    let records = sqlx::query_as::<_, EventRegistryRecord>(
        r#"
        SELECT id, abi_id, event_name, signature, topic0, inputs, indexed_inputs, anonymous, created_at
        FROM event_registry
        ORDER BY event_name, signature
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(response::success(records))
}

pub async fn find_abi(pool: &sqlx::PgPool, id: Uuid) -> Result<AbiVersionRecord, ApplicationError> {
    sqlx::query_as::<_, AbiVersionRecord>(
        r#"
        SELECT id, name, version, abi_json, status, event_count, created_at
        FROM abi_versions
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("abi {id}")))
}

pub async fn find_event_by_topic(
    pool: &sqlx::PgPool,
    abi_id: Uuid,
    topic0: &str,
) -> Result<Option<EventRegistryRecord>, ApplicationError> {
    let topic0 = normalize_topic(topic0)?;
    let record = sqlx::query_as::<_, EventRegistryRecord>(
        r#"
        SELECT id, abi_id, event_name, signature, topic0, inputs, indexed_inputs, anonymous, created_at
        FROM event_registry
        WHERE abi_id = $1 AND topic0 = $2
        "#,
    )
    .bind(abi_id)
    .bind(topic0)
    .fetch_optional(pool)
    .await?;

    Ok(record)
}

pub fn parse_abi_from_value(value: &Value) -> Result<JsonAbi, ApplicationError> {
    let abi_json_text = serde_json::to_string(value)
        .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;
    JsonAbi::from_json_str(&abi_json_text)
        .map_err(|error| ApplicationError::BadRequest(format!("invalid ABI JSON: {error}")))
}

fn event_topic0(event: &Event) -> String {
    format!("{:#x}", event.selector()).to_ascii_lowercase()
}

async fn insert_event_registry_record(
    pool: &sqlx::PgPool,
    abi_id: Uuid,
    event: &Event,
) -> Result<(), ApplicationError> {
    let inputs = serde_json::to_value(&event.inputs)
        .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;
    let indexed_inputs = serde_json::to_value(
        event
            .inputs
            .iter()
            .filter(|input| input.indexed)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| ApplicationError::BadRequest(error.to_string()))?;

    sqlx::query(
        r#"
        INSERT INTO event_registry (
            id, abi_id, event_name, signature, topic0, inputs, indexed_inputs, anonymous
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (abi_id, topic0) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(abi_id)
    .bind(&event.name)
    .bind(event.signature())
    .bind(event_topic0(event))
    .bind(inputs)
    .bind(indexed_inputs)
    .bind(event.anonymous)
    .execute(pool)
    .await?;

    Ok(())
}
