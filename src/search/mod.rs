use axum::{Json, Router, extract::State, routing::post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, QueryBuilder};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::AuthenticatedPrincipal,
    shared::{
        error::ApplicationError,
        pagination::PageRequest,
        validation::{normalize_address, normalize_topic},
    },
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        // Legacy decoded-event search. New raw-event-lake ingestion does not populate it.
        .route("/api/search", post(search_events))
        .route("/api/raw-logs/search", post(search_raw_logs))
}

#[derive(OpenApi)]
#[openapi(
    paths(search_events, search_raw_logs),
    components(schemas(
        SearchRequest,
        SearchFilter,
        SearchOperator,
        SearchSort,
        SearchEventRecord,
        RawLogSearchRequest,
        RawLogRecord
    ))
)]
struct SearchApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    SearchApiDocumentation::openapi()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchRequest {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub filters: Vec<SearchFilter>,
    pub sort: Option<SearchSort>,
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

#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct SearchEventRecord {
    pub id: Uuid,
    pub raw_log_id: Uuid,
    pub block_number: i64,
    pub chain_id: i64,
    pub contract_address: String,
    pub event_name: String,
    pub topic0: String,
    pub indexed_fields: Value,
    pub non_indexed_fields: Value,
    pub decoded_at: DateTime<Utc>,
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
    path = "/api/search",
    tag = "search",
    request_body = SearchRequest,
    responses(
        (status = 200, description = "Search results", body = ApiResponse<Vec<SearchEventRecord>>),
        (status = 400, description = "Invalid search request")
    )
)]
async fn search_events(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<ApiResponse<Vec<SearchEventRecord>>>, ApplicationError> {
    validate_search_request(&request)?;
    let page = PageRequest {
        page: request.page,
        limit: request.limit,
    };
    let meta = page.normalized();
    #[cfg(feature = "clickhouse")]
    let results = if state.configuration.clickhouse.enabled {
        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        crate::clickhouse::search_events(&client, &request, meta.limit, page.offset())
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!("ClickHouse search failed: {error}"))
            })?
    } else {
        execute_postgres_search(&state.pool, &request, meta.limit, page.offset()).await?
    };

    #[cfg(not(feature = "clickhouse"))]
    let results = execute_postgres_search(&state.pool, &request, meta.limit, page.offset()).await?;

    Ok(response::success_with_meta(
        results,
        json!({ "page": meta.page, "limit": meta.limit }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/raw-logs/search",
    tag = "search",
    request_body = RawLogSearchRequest,
    responses(
        (status = 200, description = "Raw EVM log results", body = ApiResponse<Vec<RawLogRecord>>),
        (status = 400, description = "Invalid raw-log search request")
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

    #[cfg(feature = "clickhouse")]
    let results = if state.configuration.clickhouse.enabled {
        let client = crate::clickhouse::active_client(&state)
            .await?
            .ok_or_else(|| {
                ApplicationError::ExternalService(
                    "ClickHouse is enabled but no client is available".to_owned(),
                )
            })?;
        crate::clickhouse::search_raw_logs(&client, &request, meta.limit, page.offset())
            .await
            .map_err(|error| {
                ApplicationError::ExternalService(format!(
                    "ClickHouse raw-log search failed: {error}"
                ))
            })?
    } else {
        execute_postgres_raw_log_search(&state.pool, &request, meta.limit, page.offset()).await?
    };

    #[cfg(not(feature = "clickhouse"))]
    let results =
        execute_postgres_raw_log_search(&state.pool, &request, meta.limit, page.offset()).await?;

    Ok(response::success_with_meta(
        results,
        json!({ "page": meta.page, "limit": meta.limit }),
    ))
}

pub fn validate_search_request(request: &SearchRequest) -> Result<(), ApplicationError> {
    for filter in &request.filters {
        validate_filter(filter)?;
    }

    if let Some(sort) = &request.sort {
        match sort.field.as_str() {
            "block_number" | "decoded_at" => {}
            other => {
                return Err(ApplicationError::BadRequest(format!(
                    "unsupported sort field: {other}"
                )));
            }
        }
    }

    Ok(())
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

async fn execute_postgres_search(
    pool: &sqlx::PgPool,
    request: &SearchRequest,
    limit: i64,
    offset: i64,
) -> Result<Vec<SearchEventRecord>, ApplicationError> {
    let mut query_builder = QueryBuilder::new(
        r#"
        SELECT d.id, d.raw_log_id, d.block_number, d.chain_id, d.contract_address,
               d.event_name, d.topic0, d.indexed_fields, d.non_indexed_fields, d.decoded_at
        FROM eventlake_decoded_events d
        WHERE d.decode_status = 'decoded'
        "#,
    );

    for filter in &request.filters {
        push_filter(&mut query_builder, filter)?;
    }

    push_sort(&mut query_builder, request.sort.as_ref())?;
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset);

    let results = query_builder
        .build_query_as::<SearchEventRecord>()
        .fetch_all(pool)
        .await?;

    Ok(results)
}

async fn execute_postgres_raw_log_search(
    pool: &sqlx::PgPool,
    request: &RawLogSearchRequest,
    limit: i64,
    offset: i64,
) -> Result<Vec<RawLogRecord>, ApplicationError> {
    let mut query_builder = QueryBuilder::new(
        r#"
        SELECT id, subscription_id, chain_id, block_number, block_hash, transaction_hash,
               transaction_index, log_index, contract_address, topics, data, removed, ingested_at
        FROM eventlake_raw_logs
        WHERE removed = false
        "#,
    );

    for filter in &request.filters {
        push_raw_log_filter(&mut query_builder, filter)?;
    }

    push_raw_log_sort(&mut query_builder, request.sort.as_ref())?;
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset);

    Ok(query_builder
        .build_query_as::<RawLogRecord>()
        .fetch_all(pool)
        .await?)
}

fn validate_filter(filter: &SearchFilter) -> Result<(), ApplicationError> {
    match filter.field.as_str() {
        "chain_id" | "contract_address" | "event_name" | "topic0" | "transaction_hash"
        | "block_number" | "address" => Ok(()),
        field if field.starts_with("field.") => {
            if field.trim_start_matches("field.").is_empty() {
                Err(ApplicationError::BadRequest(
                    "field filter must include a field name".to_owned(),
                ))
            } else {
                Ok(())
            }
        }
        other => Err(ApplicationError::BadRequest(format!(
            "unsupported search field: {other}"
        ))),
    }
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

fn push_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    match filter.field.as_str() {
        "chain_id" => push_i64_filter(query_builder, "d.chain_id", filter),
        "block_number" => push_i64_filter(query_builder, "d.block_number", filter),
        "contract_address" => {
            let value = string_value(&filter.value)?;
            let normalized = normalize_address(&value)?;
            push_text_filter(
                query_builder,
                "d.contract_address",
                &filter.operator,
                normalized,
            )
        }
        "event_name" => push_text_filter(
            query_builder,
            "d.event_name",
            &filter.operator,
            string_value(&filter.value)?,
        ),
        "topic0" => push_text_filter(
            query_builder,
            "d.topic0",
            &filter.operator,
            string_value(&filter.value)?,
        ),
        "transaction_hash" => push_transaction_filter(query_builder, filter),
        "address" => push_address_filter(query_builder, filter),
        field if field.starts_with("field.") => {
            let field_name = field.trim_start_matches("field.");
            push_event_field_filter(query_builder, field_name, filter)
        }
        _ => Err(ApplicationError::BadRequest("invalid filter".to_owned())),
    }
}

fn push_raw_log_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    match filter.field.as_str() {
        "chain_id" => push_i64_filter(query_builder, "chain_id", filter),
        "block_number" => push_i64_filter(query_builder, "block_number", filter),
        "contract_address" => {
            let normalized = normalize_address(&string_value(&filter.value)?)?;
            push_text_filter(
                query_builder,
                "contract_address",
                &filter.operator,
                normalized,
            )
        }
        "transaction_hash" => {
            if !matches!(filter.operator, SearchOperator::Eq) {
                return Err(ApplicationError::BadRequest(
                    "transaction_hash currently supports eq".to_owned(),
                ));
            }
            query_builder
                .push(" AND lower(transaction_hash) = ")
                .push_bind(string_value(&filter.value)?.to_ascii_lowercase());
            Ok(())
        }
        "topic0" | "topic1" | "topic2" | "topic3" => {
            let topic_index = filter
                .field
                .trim_start_matches("topic")
                .parse::<i32>()
                .expect("raw-log topic filters only expose topic0 through topic3");
            let topic = normalize_topic(&string_value(&filter.value)?)?;
            query_builder
                .push(" AND topics ->> ")
                .push_bind(topic_index)
                .push(" = ")
                .push_bind(topic);
            Ok(())
        }
        _ => Err(ApplicationError::BadRequest(
            "invalid raw-log filter".to_owned(),
        )),
    }
}

fn push_i64_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    column: &'static str,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let value = filter
        .value
        .as_i64()
        .ok_or_else(|| ApplicationError::BadRequest(format!("{} must be integer", filter.field)))?;

    match filter.operator {
        SearchOperator::Eq => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" = ")
                .push_bind(value);
        }
        SearchOperator::Neq => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" <> ")
                .push_bind(value);
        }
        SearchOperator::Gt => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" > ")
                .push_bind(value);
        }
        SearchOperator::Gte => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" >= ")
                .push_bind(value);
        }
        SearchOperator::Lt => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" < ")
                .push_bind(value);
        }
        SearchOperator::Lte => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" <= ")
                .push_bind(value);
        }
        _ => {
            return Err(ApplicationError::BadRequest(format!(
                "operator not supported for integer field: {:?}",
                filter.operator
            )));
        }
    }

    Ok(())
}

fn push_text_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    column: &'static str,
    operator: &SearchOperator,
    value: String,
) -> Result<(), ApplicationError> {
    match operator {
        SearchOperator::Eq => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" = ")
                .push_bind(value);
        }
        SearchOperator::Neq => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" <> ")
                .push_bind(value);
        }
        SearchOperator::Contains => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" ILIKE ")
                .push_bind(format!("%{value}%"));
        }
        SearchOperator::StartsWith => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" ILIKE ")
                .push_bind(format!("{value}%"));
        }
        SearchOperator::EndsWith => {
            query_builder
                .push(" AND ")
                .push(column)
                .push(" ILIKE ")
                .push_bind(format!("%{value}"));
        }
        SearchOperator::In | SearchOperator::NotIn => {
            return Err(ApplicationError::BadRequest(
                "in/not_in require array handling and are not available for this field yet"
                    .to_owned(),
            ));
        }
        _ => {
            return Err(ApplicationError::BadRequest(
                "operator not supported for text field".to_owned(),
            ));
        }
    }

    Ok(())
}

fn push_transaction_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let value = string_value(&filter.value)?.to_ascii_lowercase();
    match filter.operator {
        SearchOperator::Eq => {
            query_builder.push(
                " AND EXISTS (SELECT 1 FROM eventlake_raw_logs rl WHERE rl.id = d.raw_log_id AND rl.block_number = d.block_number AND lower(rl.transaction_hash) = "
            );
            query_builder.push_bind(value);
            query_builder.push(")");
            Ok(())
        }
        _ => Err(ApplicationError::BadRequest(
            "transaction_hash currently supports eq".to_owned(),
        )),
    }
}

fn push_address_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let value = normalize_address(&string_value(&filter.value)?)?;
    match filter.operator {
        SearchOperator::Eq => {
            query_builder.push(
                " AND EXISTS (SELECT 1 FROM eventlake_address_index ai WHERE ai.raw_log_id = d.raw_log_id AND ai.block_number = d.block_number AND ai.address = "
            );
            query_builder.push_bind(value);
            query_builder.push(")");
            Ok(())
        }
        _ => Err(ApplicationError::BadRequest(
            "address currently supports eq".to_owned(),
        )),
    }
}

fn push_event_field_filter(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    field_name: &str,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let value = string_value(&filter.value)?.to_ascii_lowercase();
    match filter.operator {
        SearchOperator::Eq => {
            query_builder.push(
                " AND EXISTS (SELECT 1 FROM eventlake_event_field_index efi WHERE efi.raw_log_id = d.raw_log_id AND efi.block_number = d.block_number AND efi.field_name = "
            );
            query_builder.push_bind(field_name.to_owned());
            query_builder.push(" AND efi.normalized_value = ");
            query_builder.push_bind(value);
            query_builder.push(")");
            Ok(())
        }
        SearchOperator::Contains => {
            query_builder.push(
                " AND EXISTS (SELECT 1 FROM eventlake_event_field_index efi WHERE efi.raw_log_id = d.raw_log_id AND efi.block_number = d.block_number AND efi.field_name = "
            );
            query_builder.push_bind(field_name.to_owned());
            query_builder.push(" AND efi.normalized_value ILIKE ");
            query_builder.push_bind(format!("%{value}%"));
            query_builder.push(")");
            Ok(())
        }
        _ => Err(ApplicationError::BadRequest(
            "field filters currently support eq and contains".to_owned(),
        )),
    }
}

fn push_raw_log_sort(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    sort: Option<&SearchSort>,
) -> Result<(), ApplicationError> {
    match sort {
        Some(sort) if sort.field == "block_number" => {
            query_builder.push(" ORDER BY block_number ");
            push_direction(query_builder, sort.direction.as_deref());
            query_builder.push(", log_index ");
            push_direction(query_builder, sort.direction.as_deref());
        }
        Some(sort) if sort.field == "log_index" => {
            query_builder.push(" ORDER BY log_index ");
            push_direction(query_builder, sort.direction.as_deref());
        }
        Some(sort) if sort.field == "ingested_at" => {
            query_builder.push(" ORDER BY ingested_at ");
            push_direction(query_builder, sort.direction.as_deref());
        }
        Some(sort) => {
            return Err(ApplicationError::BadRequest(format!(
                "unsupported raw-log sort field: {}",
                sort.field
            )));
        }
        None => {
            query_builder.push(" ORDER BY block_number DESC, log_index DESC");
        }
    }

    Ok(())
}

fn push_sort(
    query_builder: &mut QueryBuilder<sqlx::Postgres>,
    sort: Option<&SearchSort>,
) -> Result<(), ApplicationError> {
    match sort {
        Some(sort) if sort.field == "block_number" => {
            query_builder.push(" ORDER BY d.block_number ");
            push_direction(query_builder, sort.direction.as_deref());
        }
        Some(sort) if sort.field == "decoded_at" => {
            query_builder.push(" ORDER BY d.decoded_at ");
            push_direction(query_builder, sort.direction.as_deref());
        }
        Some(sort) => {
            return Err(ApplicationError::BadRequest(format!(
                "unsupported sort field: {}",
                sort.field
            )));
        }
        None => {
            query_builder.push(" ORDER BY d.block_number DESC, d.decoded_at DESC");
        }
    }

    Ok(())
}

fn push_direction(query_builder: &mut QueryBuilder<sqlx::Postgres>, direction: Option<&str>) {
    if matches!(direction, Some("asc") | Some("ASC")) {
        query_builder.push("ASC");
    } else {
        query_builder.push("DESC");
    }
}

fn string_value(value: &Value) -> Result<String, ApplicationError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ApplicationError::BadRequest("filter value must be a string".to_owned()))
}
