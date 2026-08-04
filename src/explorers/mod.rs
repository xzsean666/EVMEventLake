use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;
use serde_json::Value;
#[cfg(feature = "clickhouse")]
use serde_json::json;
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::{error::ApplicationError, validation::normalize_address},
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route("/api/explorer/address/{address}", get(address_explorer))
        .route(
            "/api/explorer/contracts/{chain_id}/{contract_address}",
            get(contract_explorer),
        )
        .route("/api/explorer/events/{event_name}", get(event_explorer))
}

#[derive(OpenApi)]
#[openapi(
    paths(address_explorer, contract_explorer, event_explorer),
    components(schemas(
        AddressExplorerResponse,
        AddressRecentEvent,
        RelatedContract,
        EventStatistic,
        ContractExplorerResponse,
        EventExplorerResponse
    ))
)]
struct ExplorersApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ExplorersApiDocumentation::openapi()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AddressExplorerResponse {
    pub address: String,
    pub recent_events: Vec<AddressRecentEvent>,
    pub related_contracts: Vec<RelatedContract>,
    pub event_statistics: Vec<EventStatistic>,
    pub last_activity_block: Option<i64>,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct AddressRecentEvent {
    pub chain_id: i64,
    pub contract_address: String,
    pub event_name: String,
    pub field_name: String,
    pub block_number: i64,
    pub transaction_hash: String,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct RelatedContract {
    pub chain_id: i64,
    pub contract_address: String,
    pub event_count: i64,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct EventStatistic {
    pub event_name: String,
    pub event_count: i64,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ContractExplorerResponse {
    pub chain_id: i64,
    pub contract_address: String,
    pub event_count: i64,
    pub first_seen_block: Option<i64>,
    pub last_seen_block: Option<i64>,
    pub event_types: Value,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct EventExplorerResponse {
    pub event_name: String,
    pub signatures: Value,
    pub topic0_values: Value,
    pub contract_count: i64,
    pub total_count: i64,
}

#[utoipa::path(
    get,
    path = "/api/explorer/address/{address}",
    tag = "explorers",
    params(("address" = String, Path, description = "EVM address")),
    responses(
        (status = 200, description = "Address explorer", body = ApiResponse<AddressExplorerResponse>),
        (status = 400, description = "Invalid address")
    )
)]
async fn address_explorer(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(address): Path<String>,
) -> Result<Json<ApiResponse<AddressExplorerResponse>>, ApplicationError> {
    let address = normalize_address(&address)?;

    #[cfg(feature = "clickhouse")]
    if state.configuration.clickhouse.enabled {
        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        let (recent_events, related_contracts, event_statistics) =
            crate::clickhouse::address_explorer(&client, &address)
                .await
                .map_err(|error| {
                    ApplicationError::ExternalService(format!(
                        "ClickHouse address explorer query failed: {error}"
                    ))
                })?;
        let recent_events = recent_events
            .into_iter()
            .map(|event| {
                Ok(AddressRecentEvent {
                    chain_id: to_i64(event.chain_id, "chain_id")?,
                    contract_address: event.contract_address,
                    event_name: event.event_name,
                    field_name: event.field_name,
                    block_number: to_i64(event.block_number, "block_number")?,
                    transaction_hash: event.transaction_hash,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        let last_activity_block = recent_events.first().map(|event| event.block_number);
        let related_contracts = related_contracts
            .into_iter()
            .map(|contract| {
                Ok(RelatedContract {
                    chain_id: to_i64(contract.chain_id, "chain_id")?,
                    contract_address: contract.contract_address,
                    event_count: to_i64(contract.event_count, "event_count")?,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        let event_statistics = event_statistics
            .into_iter()
            .map(|event| {
                Ok(EventStatistic {
                    event_name: event.event_name,
                    event_count: to_i64(event.event_count, "event_count")?,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;

        return Ok(response::success(AddressExplorerResponse {
            address,
            recent_events,
            related_contracts,
            event_statistics,
            last_activity_block,
        }));
    }

    let recent_events = sqlx::query_as::<_, AddressRecentEvent>(
        r#"
        SELECT chain_id, contract_address, event_name, field_name, block_number, transaction_hash
        FROM eventlake_address_index
        WHERE address = $1
        ORDER BY block_number DESC
        LIMIT 50
        "#,
    )
    .bind(&address)
    .fetch_all(&state.pool)
    .await?;

    let related_contracts = sqlx::query_as::<_, RelatedContract>(
        r#"
        SELECT chain_id, contract_address, COUNT(*)::BIGINT AS event_count
        FROM eventlake_address_index
        WHERE address = $1
        GROUP BY chain_id, contract_address
        ORDER BY event_count DESC
        LIMIT 50
        "#,
    )
    .bind(&address)
    .fetch_all(&state.pool)
    .await?;

    let event_statistics = sqlx::query_as::<_, EventStatistic>(
        r#"
        SELECT event_name, COUNT(*)::BIGINT AS event_count
        FROM eventlake_address_index
        WHERE address = $1
        GROUP BY event_name
        ORDER BY event_count DESC
        LIMIT 50
        "#,
    )
    .bind(&address)
    .fetch_all(&state.pool)
    .await?;

    let last_activity_block = recent_events.first().map(|event| event.block_number);

    Ok(response::success(AddressExplorerResponse {
        address,
        recent_events,
        related_contracts,
        event_statistics,
        last_activity_block,
    }))
}

#[utoipa::path(
    get,
    path = "/api/explorer/contracts/{chain_id}/{contract_address}",
    tag = "explorers",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("contract_address" = String, Path, description = "Contract address")
    ),
    responses(
        (status = 200, description = "Contract explorer", body = ApiResponse<ContractExplorerResponse>),
        (status = 400, description = "Invalid contract address"),
        (status = 404, description = "Contract not found")
    )
)]
async fn contract_explorer(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, contract_address)): Path<(i64, String)>,
) -> Result<Json<ApiResponse<ContractExplorerResponse>>, ApplicationError> {
    let contract_address = normalize_address(&contract_address)?;

    #[cfg(feature = "clickhouse")]
    if state.configuration.clickhouse.enabled {
        let exists = sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT chain_id
            FROM eventlake_contract_registry
            WHERE chain_id = $1 AND contract_address = $2
            "#,
        )
        .bind(chain_id)
        .bind(&contract_address)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
        if !exists {
            return Err(ApplicationError::NotFound(format!(
                "contract {chain_id}:{contract_address}"
            )));
        }

        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        let statistics = crate::clickhouse::contract_explorer(&client, chain_id, &contract_address)
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!(
                    "ClickHouse contract explorer query failed: {error}"
                ))
            })?;
        return Ok(response::success(ContractExplorerResponse {
            chain_id,
            contract_address,
            event_count: to_i64(statistics.event_count, "event_count")?,
            first_seen_block: statistics
                .first_seen_block
                .map(|block_number| to_i64(block_number, "first_seen_block"))
                .transpose()?,
            last_seen_block: statistics
                .last_seen_block
                .map(|block_number| to_i64(block_number, "last_seen_block"))
                .transpose()?,
            event_types: json!(statistics.event_types),
        }));
    }

    let record = sqlx::query_as::<_, ContractExplorerResponse>(
        r#"
        SELECT cr.chain_id,
               cr.contract_address,
               COUNT(de.id)::BIGINT AS event_count,
               MIN(de.block_number) AS first_seen_block,
               MAX(de.block_number) AS last_seen_block,
               COALESCE(jsonb_agg(DISTINCT de.event_name) FILTER (WHERE de.event_name IS NOT NULL), '[]'::jsonb) AS event_types
        FROM eventlake_contract_registry cr
        LEFT JOIN eventlake_decoded_events de
          ON de.chain_id = cr.chain_id
         AND de.contract_address = cr.contract_address
         AND de.decode_status = 'decoded'
        WHERE cr.chain_id = $1 AND cr.contract_address = $2
        GROUP BY cr.chain_id, cr.contract_address
        "#,
    )
    .bind(chain_id)
    .bind(&contract_address)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound(format!("contract {chain_id}:{contract_address}")))?;

    Ok(response::success(record))
}

#[utoipa::path(
    get,
    path = "/api/explorer/events/{event_name}",
    tag = "explorers",
    params(("event_name" = String, Path, description = "Event name")),
    responses(
        (status = 200, description = "Event explorer", body = ApiResponse<EventExplorerResponse>),
        (status = 404, description = "Event not found")
    )
)]
async fn event_explorer(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(event_name): Path<String>,
) -> Result<Json<ApiResponse<EventExplorerResponse>>, ApplicationError> {
    #[cfg(feature = "clickhouse")]
    if state.configuration.clickhouse.enabled {
        let (signatures, topic0_values) = sqlx::query_as::<_, (Value, Value)>(
            r#"
            SELECT jsonb_agg(DISTINCT signature) AS signatures,
                   jsonb_agg(DISTINCT topic0) AS topic0_values
            FROM eventlake_event_registry
            WHERE event_name = $1
            GROUP BY event_name
            "#,
        )
        .bind(&event_name)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApplicationError::NotFound("event".to_owned()))?;

        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        let statistics = crate::clickhouse::event_explorer(&client, &event_name)
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!(
                    "ClickHouse event explorer query failed: {error}"
                ))
            })?;

        return Ok(response::success(EventExplorerResponse {
            event_name,
            signatures,
            topic0_values,
            contract_count: to_i64(statistics.contract_count, "contract_count")?,
            total_count: to_i64(statistics.total_count, "total_count")?,
        }));
    }

    let record = sqlx::query_as::<_, EventExplorerResponse>(
        r#"
        SELECT er.event_name,
               jsonb_agg(DISTINCT er.signature) AS signatures,
               jsonb_agg(DISTINCT er.topic0) AS topic0_values,
               COUNT(DISTINCT de.contract_address)::BIGINT AS contract_count,
               COUNT(DISTINCT de.id)::BIGINT AS total_count
        FROM eventlake_event_registry er
        LEFT JOIN eventlake_decoded_events de
          ON de.topic0 = er.topic0
         AND de.decode_status = 'decoded'
        WHERE er.event_name = $1
        GROUP BY er.event_name
        "#,
    )
    .bind(event_name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApplicationError::NotFound("event".to_owned()))?;

    Ok(response::success(record))
}

#[cfg(feature = "clickhouse")]
fn to_i64(value: u64, field: &str) -> Result<i64, ApplicationError> {
    i64::try_from(value).map_err(|_| {
        ApplicationError::ExternalService(format!("ClickHouse {field} exceeds PostgreSQL BIGINT"))
    })
}
