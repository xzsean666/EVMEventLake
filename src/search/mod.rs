use axum::{Json, Router, extract::State, routing::post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::{
        error::ApplicationError,
        pagination::PageRequest,
        validation::normalize_topic,
    },
};

pub fn routes() -> Router<ApplicationState> {
    Router::new().route("/api/raw-logs/search", post(search_raw_logs))
}

#[derive(OpenApi)]
#[openapi(
    paths(search_raw_logs),
    components(schemas(
        SearchFilter,
        SearchOperator,
        SearchSort,
        RawLogSearchRequest,
        RawLogRecord
    ))
)]
struct SearchApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    SearchApiDocumentation::openapi()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchFilter {
    pub field: String,
    pub operator: SearchOperator,
    pub value: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    StartsWith,
    EndsWith,
    In,
    NotIn,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchSort {
    pub field: String,
    pub direction: Option<String>,
}

/// Raw-log search deliberately operates on encoded EVM values. `topic0` through
/// `topic3` are positional topics; missing positions simply never match.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RawLogSearchRequest {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub filters: Vec<SearchFilter>,
    pub sort: Option<SearchSort>,
}

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct RawLogRecord {
    pub id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: i64,
    pub log_index: i64,
    pub contract_address: String,
    pub topics: Value,
    pub data: String,
    pub removed: bool,
    pub ingested_at: DateTime<Utc>,
}

#[utoipa::path(
    post,
    path = "/api/raw-logs/search",
    tag = "search",
    request_body = RawLogSearchRequest,
    responses(
        (status = 200, description = "Raw log search results", body = ApiResponse<Vec<RawLogRecord>>),
        (status = 400, description = "Invalid search request")
    )
)]
async fn search_raw_logs(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<RawLogSearchRequest>,
) -> Result<Json<ApiResponse<Vec<RawLogRecord>>>, ApplicationError> {
    validate_raw_log_search_request(&request)?;
    let page = PageRequest {
        page: request.page,
        limit: request.limit,
    };
    let meta = page.normalized();
    let offset = page.offset();

    let client = crate::clickhouse::active_client(&state)
        .await?
        .ok_or_else(|| {
            ApplicationError::ExternalService(
                "ClickHouse is required as raw-event lake but no client is available".to_owned(),
            )
        })?;
    let results = crate::clickhouse::search_raw_logs(&client, &request, meta.limit, offset).await?;

    Ok(response::success_with_meta(
        results,
        json!({ "page": meta.page, "limit": meta.limit }),
    ))
}

pub fn validate_raw_log_search_request(
    request: &RawLogSearchRequest,
) -> Result<(), ApplicationError> {
    let has_chain_id = request.filters.iter().any(|filter| {
        filter.field == "chain_id"
            && matches!(filter.operator, SearchOperator::Eq)
            && filter.value.as_i64().is_some_and(|value| value > 0)
    });
    if !has_chain_id {
        return Err(ApplicationError::BadRequest(
            "raw-log search requires a positive chain_id eq filter".to_owned(),
        ));
    }

    for filter in &request.filters {
        validate_raw_log_filter(filter)?;
    }

    if let Some(sort) = &request.sort {
        match sort.field.as_str() {
            "block_number" | "log_index" | "ingested_at" => {}
            other => {
                return Err(ApplicationError::BadRequest(format!(
                    "unsupported raw-log sort field: {other}"
                )));
            }
        }
    }

    Ok(())
}

fn validate_raw_log_filter(filter: &SearchFilter) -> Result<(), ApplicationError> {
    match filter.field.as_str() {
        "chain_id" => {
            let value = filter.value.as_i64().ok_or_else(|| {
                ApplicationError::BadRequest("chain_id must be an integer".to_owned())
            })?;
            if value <= 0 {
                return Err(ApplicationError::BadRequest(
                    "chain_id must be greater than 0".to_owned(),
                ));
            }
            Ok(())
        }
        "block_number" => {
            let value = filter.value.as_i64().ok_or_else(|| {
                ApplicationError::BadRequest("block_number must be an integer".to_owned())
            })?;
            if value < 0 {
                return Err(ApplicationError::BadRequest(
                    "block_number must be greater than or equal to 0".to_owned(),
                ));
            }
            Ok(())
        }
        "contract_address" | "transaction_hash" => Ok(()),
        "topic0" | "topic1" | "topic2" | "topic3" => {
            if !matches!(filter.operator, SearchOperator::Eq) {
                return Err(ApplicationError::BadRequest(format!(
                    "{} currently supports eq",
                    filter.field
                )));
            }
            normalize_topic(&string_value(&filter.value)?)?;
            Ok(())
        }
        other => Err(ApplicationError::BadRequest(format!(
            "unsupported raw-log search field: {other}"
        ))),
    }
}

fn string_value(value: &Value) -> Result<String, ApplicationError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ApplicationError::BadRequest("filter value must be a string".to_owned()))
}
