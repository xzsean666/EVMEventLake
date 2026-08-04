use anyhow::Context;
use chrono::{DateTime, Utc};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    configuration::ClickHouseConfig,
    indexing::DecodedFieldValue,
    search::{SearchEventRecord, SearchFilter, SearchOperator, SearchRequest, SearchSort},
    shared::{error::ApplicationError, validation::normalize_address},
};

const SCHEMA: &str = include_str!("../../clickhouse/schema.sql");

#[derive(Clone, Debug)]
pub struct IndexedEvent {
    pub id: Uuid,
    pub raw_log_id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub log_index: i64,
    pub contract_address: String,
    pub event_name: String,
    pub topic0: String,
    pub abi_id: Option<Uuid>,
    pub indexed_fields: Value,
    pub non_indexed_fields: Value,
    pub fields: Vec<DecodedFieldValue>,
    pub is_removed: bool,
    pub decoded_at: DateTime<Utc>,
}

#[derive(Row, Serialize)]
struct DecodedEventRow {
    #[serde(with = "clickhouse::serde::uuid")]
    id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    raw_log_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    subscription_id: Option<Uuid>,
    chain_id: u64,
    block_number: u64,
    block_hash: String,
    transaction_hash: String,
    log_index: u32,
    contract_address: String,
    event_name: String,
    topic0: String,
    #[serde(with = "clickhouse::serde::uuid::option")]
    abi_id: Option<Uuid>,
    indexed_fields: String,
    non_indexed_fields: String,
    decoded_fields: String,
    is_removed: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    decoded_at: OffsetDateTime,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    indexed_at: OffsetDateTime,
}

#[derive(Row, Serialize)]
struct AddressIndexRow {
    chain_id: u64,
    address: String,
    block_number: u64,
    transaction_hash: String,
    log_index: u32,
    event_name: String,
    contract_address: String,
    role: String,
    field_name: String,
    is_removed: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    indexed_at: OffsetDateTime,
}

#[derive(Row, Serialize)]
struct EventFieldIndexRow {
    chain_id: u64,
    topic0: String,
    field_name: String,
    field_value: String,
    block_number: u64,
    transaction_hash: String,
    log_index: u32,
    is_removed: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    indexed_at: OffsetDateTime,
}

pub async fn connect(configuration: &ClickHouseConfig) -> anyhow::Result<Option<Client>> {
    if !configuration.enabled {
        tracing::info!("ClickHouse disabled by EVENTLAKE_CLICKHOUSE_ENABLED");
        return Ok(None);
    }

    let client = Client::default()
        .with_url(configuration.url())
        .with_user(configuration.user.clone())
        .with_password(configuration.password.clone())
        .with_database(configuration.database.clone());

    client
        .query("SELECT 1")
        .fetch_one::<u8>()
        .await
        .context("ClickHouse healthcheck failed")?;
    initialize_schema(&client).await?;

    tracing::info!(
        host = %configuration.host,
        port = configuration.port,
        database = %configuration.database,
        "ClickHouse connected"
    );
    Ok(Some(client))
}

pub async fn initialize_schema(client: &Client) -> anyhow::Result<()> {
    for statement in SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|sql| !sql.is_empty())
    {
        client
            .query(statement)
            .execute()
            .await
            .with_context(|| format!("failed to apply ClickHouse DDL: {statement}"))?;
    }

    Ok(())
}

/// Mirrors one committed PostgreSQL decoded event. The caller owns the source-of-truth
/// transaction and invokes this only after it commits, so a replica failure never rolls
/// PostgreSQL state back.
pub async fn write_indexed_event(client: &Client, event: IndexedEvent) -> anyhow::Result<()> {
    let chain_id = as_u64(event.chain_id, "chain_id")?;
    let block_number = as_u64(event.block_number, "block_number")?;
    let log_index = as_u32(event.log_index, "log_index")?;
    let indexed_at = Utc::now();
    let indexed_at_offset = to_offset_datetime(indexed_at)?;
    let decoded_fields = json!({
        "indexed": event.indexed_fields.clone(),
        "non_indexed": event.non_indexed_fields.clone(),
    })
    .to_string();

    let main_row = DecodedEventRow {
        id: event.id,
        raw_log_id: event.raw_log_id,
        subscription_id: event.subscription_id,
        chain_id,
        block_number,
        block_hash: event.block_hash,
        transaction_hash: event.transaction_hash.clone(),
        log_index,
        contract_address: event.contract_address.clone(),
        event_name: event.event_name.clone(),
        topic0: event.topic0.clone(),
        abi_id: event.abi_id,
        indexed_fields: event.indexed_fields.to_string(),
        non_indexed_fields: event.non_indexed_fields.to_string(),
        decoded_fields,
        is_removed: event.is_removed,
        decoded_at: to_offset_datetime(event.decoded_at)?,
        indexed_at: indexed_at_offset,
    };
    write_rows(client, "decoded_events", &[main_row]).await?;

    let mut address_rows = Vec::new();
    let mut field_rows = Vec::new();
    for field in event.fields {
        if let Ok(address) = crate::shared::validation::normalize_address(&field.normalized_value) {
            address_rows.push(AddressIndexRow {
                chain_id,
                address,
                block_number,
                transaction_hash: event.transaction_hash.clone(),
                log_index,
                event_name: event.event_name.clone(),
                contract_address: event.contract_address.clone(),
                role: "field".to_owned(),
                field_name: field.field_name.clone(),
                is_removed: event.is_removed,
                indexed_at: indexed_at_offset,
            });
        }

        if should_index_field_value(&field.json_value) {
            field_rows.push(EventFieldIndexRow {
                chain_id,
                topic0: event.topic0.clone(),
                field_name: field.field_name,
                field_value: field.normalized_value,
                block_number,
                transaction_hash: event.transaction_hash.clone(),
                log_index,
                is_removed: event.is_removed,
                indexed_at: indexed_at_offset,
            });
        }
    }

    if !address_rows.is_empty() {
        write_rows(client, "address_index", &address_rows).await?;
    }
    if !field_rows.is_empty() {
        write_rows(client, "event_field_index", &field_rows).await?;
    }

    Ok(())
}

#[derive(FromRow)]
struct ReorgEventRow {
    id: Uuid,
    raw_log_id: Uuid,
    subscription_id: Option<Uuid>,
    chain_id: i64,
    block_number: i64,
    block_hash: String,
    transaction_hash: String,
    log_index: i64,
    contract_address: String,
    abi_id: Option<Uuid>,
    event_name: String,
    topic0: String,
    indexed_fields: Value,
    non_indexed_fields: Value,
    decoded_at: DateTime<Utc>,
}

/// Mirrors PostgreSQL's reorg invalidation as newer tombstone rows. `FINAL` on the
/// ClickHouse search path makes the invalidation visible before background merges run.
pub async fn sync_reorg_from_postgres(
    client: &Client,
    pool: &sqlx::PgPool,
    chain_id: i64,
    from_block: i64,
) -> anyhow::Result<usize> {
    let rows = sqlx::query_as::<_, ReorgEventRow>(
        r#"
        SELECT d.id,
               d.raw_log_id,
               rl.subscription_id,
               d.chain_id,
               d.block_number,
               rl.block_hash,
               rl.transaction_hash,
               rl.log_index,
               d.contract_address,
               d.abi_id,
               d.event_name,
               d.topic0,
               d.indexed_fields,
               d.non_indexed_fields,
               d.decoded_at
        FROM eventlake_decoded_events d
        JOIN eventlake_raw_logs rl
          ON rl.id = d.raw_log_id AND rl.block_number = d.block_number
        WHERE d.chain_id = $1
          AND d.block_number >= $2
          AND d.decode_status = 'reorged'
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .fetch_all(pool)
    .await
    .context("failed to read PostgreSQL reorg rows for ClickHouse sync")?;

    let count = rows.len();
    for row in rows {
        write_indexed_event(
            client,
            IndexedEvent {
                id: row.id,
                raw_log_id: row.raw_log_id,
                subscription_id: row.subscription_id,
                chain_id: row.chain_id,
                block_number: row.block_number,
                block_hash: row.block_hash,
                transaction_hash: row.transaction_hash,
                log_index: row.log_index,
                contract_address: row.contract_address,
                event_name: row.event_name,
                topic0: row.topic0,
                abi_id: row.abi_id,
                indexed_fields: row.indexed_fields.clone(),
                non_indexed_fields: row.non_indexed_fields.clone(),
                fields: decoded_fields_from_json(&row.indexed_fields, &row.non_indexed_fields),
                is_removed: true,
                decoded_at: row.decoded_at,
            },
        )
        .await?;
    }

    Ok(count)
}

async fn write_rows<T>(client: &Client, table: &str, rows: &[T]) -> anyhow::Result<()>
where
    T: Row + Serialize,
{
    let mut insert = client
        .insert(table)
        .with_context(|| format!("failed to start ClickHouse insert into {table}"))?;
    for row in rows {
        insert
            .write(row)
            .await
            .with_context(|| format!("failed to write ClickHouse row into {table}"))?;
    }
    insert
        .end()
        .await
        .with_context(|| format!("failed to finish ClickHouse insert into {table}"))?;

    Ok(())
}

fn should_index_field_value(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

fn decoded_fields_from_json(
    indexed_fields: &Value,
    non_indexed_fields: &Value,
) -> Vec<DecodedFieldValue> {
    let mut fields = Vec::new();
    for object in [indexed_fields, non_indexed_fields] {
        if let Some(values) = object.as_object() {
            for (field_name, json_value) in values {
                fields.push(DecodedFieldValue {
                    field_name: field_name.clone(),
                    field_type: "unknown".to_owned(),
                    normalized_value: normalized_json_value(json_value),
                    json_value: json_value.clone(),
                });
            }
        }
    }
    fields
}

fn normalized_json_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.to_ascii_lowercase(),
        other => other.to_string().to_ascii_lowercase(),
    }
}

fn as_u64(value: i64, field: &str) -> anyhow::Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} cannot be negative"))
}

fn to_offset_datetime(value: DateTime<Utc>) -> anyhow::Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(
        value
            .timestamp_nanos_opt()
            .context("timestamp is outside the supported range")? as i128,
    )
    .context("timestamp cannot be represented by ClickHouse time mapping")
}

fn as_u32(value: i64, field: &str) -> anyhow::Result<u32> {
    u32::try_from(value).with_context(|| format!("{field} is outside ClickHouse UInt32 range"))
}

#[derive(Row, Deserialize)]
struct SearchRow {
    #[serde(with = "clickhouse::serde::uuid")]
    id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    raw_log_id: Uuid,
    block_number: u64,
    chain_id: u64,
    contract_address: String,
    event_name: String,
    topic0: String,
    indexed_fields: String,
    non_indexed_fields: String,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    decoded_at: OffsetDateTime,
}

enum QueryArgument {
    Text(String),
    Signed(i64),
    Unsigned(u64),
}

struct SearchQuery {
    sql: String,
    arguments: Vec<QueryArgument>,
}

pub async fn search_events(
    client: &Client,
    request: &SearchRequest,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<SearchEventRecord>> {
    let search = build_search_query(request, limit, offset)?;
    let mut query = client.query(&search.sql);
    for argument in search.arguments {
        query = match argument {
            QueryArgument::Text(value) => query.bind(value),
            QueryArgument::Signed(value) => query.bind(value),
            QueryArgument::Unsigned(value) => query.bind(value),
        };
    }

    let rows = query.fetch_all::<SearchRow>().await?;
    rows.into_iter()
        .map(|row| {
            Ok(SearchEventRecord {
                id: row.id,
                raw_log_id: row.raw_log_id,
                block_number: i64::try_from(row.block_number)
                    .context("ClickHouse block number exceeds PostgreSQL BIGINT")?,
                chain_id: i64::try_from(row.chain_id)
                    .context("ClickHouse chain ID exceeds PostgreSQL BIGINT")?,
                contract_address: row.contract_address,
                event_name: row.event_name,
                topic0: row.topic0,
                indexed_fields: serde_json::from_str(&row.indexed_fields)
                    .context("invalid indexed_fields JSON in ClickHouse")?,
                non_indexed_fields: serde_json::from_str(&row.non_indexed_fields)
                    .context("invalid non_indexed_fields JSON in ClickHouse")?,
                decoded_at: chrono_from_offset_datetime(row.decoded_at)?,
            })
        })
        .collect()
}

fn chrono_from_offset_datetime(value: OffsetDateTime) -> anyhow::Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(value.unix_timestamp(), value.nanosecond())
        .context("ClickHouse timestamp cannot be converted to chrono")
}

fn build_search_query(
    request: &SearchRequest,
    limit: i64,
    offset: i64,
) -> Result<SearchQuery, ApplicationError> {
    let mut query = SearchQuery {
        sql: String::from(
            r#"
            SELECT id, raw_log_id, block_number, chain_id, contract_address,
                   event_name, topic0, indexed_fields, non_indexed_fields, decoded_at
            FROM decoded_events FINAL
            WHERE is_removed = false
            "#,
        ),
        arguments: Vec::new(),
    };

    for filter in &request.filters {
        push_search_filter(&mut query, filter)?;
    }

    push_search_sort(&mut query, request.sort.as_ref())?;
    query.sql.push_str(" LIMIT ? OFFSET ?");
    query
        .arguments
        .push(QueryArgument::Unsigned(u64::try_from(limit).map_err(
            |_| ApplicationError::BadRequest("search limit cannot be negative".to_owned()),
        )?));
    query
        .arguments
        .push(QueryArgument::Unsigned(u64::try_from(offset).map_err(
            |_| ApplicationError::BadRequest("search offset cannot be negative".to_owned()),
        )?));

    Ok(query)
}

fn push_search_filter(
    query: &mut SearchQuery,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    match filter.field.as_str() {
        "chain_id" => push_integer_filter(query, "chain_id", filter),
        "block_number" => push_integer_filter(query, "block_number", filter),
        "contract_address" => {
            let normalized = normalize_address(&string_value(&filter.value)?)?;
            push_text_filter(query, "contract_address", &filter.operator, normalized)
        }
        "event_name" => push_text_filter(
            query,
            "event_name",
            &filter.operator,
            string_value(&filter.value)?,
        ),
        "topic0" => push_text_filter(
            query,
            "topic0",
            &filter.operator,
            string_value(&filter.value)?,
        ),
        "transaction_hash" => push_transaction_filter(query, filter),
        "address" => push_address_filter(query, filter),
        field if field.starts_with("field.") => {
            push_event_field_filter(query, field.trim_start_matches("field."), filter)
        }
        _ => Err(ApplicationError::BadRequest("invalid filter".to_owned())),
    }
}

fn push_integer_filter(
    query: &mut SearchQuery,
    column: &'static str,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let value = filter
        .value
        .as_i64()
        .ok_or_else(|| ApplicationError::BadRequest(format!("{} must be integer", filter.field)))?;
    let operator = match filter.operator {
        SearchOperator::Eq => "=",
        SearchOperator::Neq => "<>",
        SearchOperator::Gt => ">",
        SearchOperator::Gte => ">=",
        SearchOperator::Lt => "<",
        SearchOperator::Lte => "<=",
        _ => {
            return Err(ApplicationError::BadRequest(format!(
                "operator not supported for integer field: {:?}",
                filter.operator
            )));
        }
    };
    query.sql.push_str(" AND ");
    query.sql.push_str(column);
    query.sql.push(' ');
    query.sql.push_str(operator);
    query.sql.push_str(" ?");
    query.arguments.push(QueryArgument::Signed(value));
    Ok(())
}

fn push_text_filter(
    query: &mut SearchQuery,
    column: &'static str,
    operator: &SearchOperator,
    value: String,
) -> Result<(), ApplicationError> {
    match operator {
        SearchOperator::Eq | SearchOperator::Neq => {
            query.sql.push_str(" AND ");
            query.sql.push_str(column);
            query
                .sql
                .push_str(if matches!(operator, SearchOperator::Eq) {
                    " = ?"
                } else {
                    " <> ?"
                });
            query.arguments.push(QueryArgument::Text(value));
        }
        SearchOperator::Contains => {
            query.sql.push_str(" AND positionCaseInsensitiveUTF8(");
            query.sql.push_str(column);
            query.sql.push_str(", ?) > 0");
            query.arguments.push(QueryArgument::Text(value));
        }
        SearchOperator::StartsWith => {
            query.sql.push_str(" AND startsWith(lowerUTF8(");
            query.sql.push_str(column);
            query.sql.push_str("), lowerUTF8(?))");
            query.arguments.push(QueryArgument::Text(value));
        }
        SearchOperator::EndsWith => {
            query.sql.push_str(" AND endsWith(lowerUTF8(");
            query.sql.push_str(column);
            query.sql.push_str("), lowerUTF8(?))");
            query.arguments.push(QueryArgument::Text(value));
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
    query: &mut SearchQuery,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    if !matches!(filter.operator, SearchOperator::Eq) {
        return Err(ApplicationError::BadRequest(
            "transaction_hash currently supports eq".to_owned(),
        ));
    }

    query.sql.push_str(" AND lowerUTF8(transaction_hash) = ?");
    query.arguments.push(QueryArgument::Text(
        string_value(&filter.value)?.to_ascii_lowercase(),
    ));
    Ok(())
}

fn push_address_filter(
    query: &mut SearchQuery,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    if !matches!(filter.operator, SearchOperator::Eq) {
        return Err(ApplicationError::BadRequest(
            "address currently supports eq".to_owned(),
        ));
    }

    query.sql.push_str(
        r#"
        AND (chain_id, transaction_hash, log_index, block_number) IN (
            SELECT chain_id, transaction_hash, log_index, block_number
            FROM address_index FINAL
            WHERE is_removed = false AND role = 'field' AND address = ?
        )
        "#,
    );
    query
        .arguments
        .push(QueryArgument::Text(normalize_address(&string_value(
            &filter.value,
        )?)?));
    Ok(())
}

fn push_event_field_filter(
    query: &mut SearchQuery,
    field_name: &str,
    filter: &SearchFilter,
) -> Result<(), ApplicationError> {
    let condition = match filter.operator {
        SearchOperator::Eq => "field_value = ?",
        SearchOperator::Contains => "positionCaseInsensitiveUTF8(field_value, ?) > 0",
        _ => {
            return Err(ApplicationError::BadRequest(
                "field filters currently support eq and contains".to_owned(),
            ));
        }
    };

    query.sql.push_str(
        r#"
        AND (chain_id, transaction_hash, log_index, block_number) IN (
            SELECT chain_id, transaction_hash, log_index, block_number
            FROM event_field_index FINAL
            WHERE is_removed = false AND field_name = ? AND "#,
    );
    query.sql.push_str(condition);
    query.sql.push_str(")");
    query
        .arguments
        .push(QueryArgument::Text(field_name.to_owned()));
    query.arguments.push(QueryArgument::Text(
        string_value(&filter.value)?.to_ascii_lowercase(),
    ));
    Ok(())
}

fn push_search_sort(
    query: &mut SearchQuery,
    sort: Option<&SearchSort>,
) -> Result<(), ApplicationError> {
    match sort {
        Some(sort) if sort.field == "block_number" || sort.field == "decoded_at" => {
            query.sql.push_str(" ORDER BY ");
            query.sql.push_str(&sort.field);
            query.sql.push(' ');
            query.sql.push_str(
                if matches!(sort.direction.as_deref(), Some("asc") | Some("ASC")) {
                    "ASC"
                } else {
                    "DESC"
                },
            );
        }
        Some(sort) => {
            return Err(ApplicationError::BadRequest(format!(
                "unsupported sort field: {}",
                sort.field
            )));
        }
        None => query
            .sql
            .push_str(" ORDER BY block_number DESC, decoded_at DESC"),
    }

    Ok(())
}

fn string_value(value: &Value) -> Result<String, ApplicationError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ApplicationError::BadRequest("filter value must be a string".to_owned()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn search_query_uses_analytical_indexes_for_address_and_fields() {
        let request = SearchRequest {
            page: Some(1),
            limit: Some(10),
            filters: vec![
                SearchFilter {
                    field: "address".to_owned(),
                    operator: SearchOperator::Eq,
                    value: json!("0x1111111111111111111111111111111111111111"),
                },
                SearchFilter {
                    field: "field.value".to_owned(),
                    operator: SearchOperator::Eq,
                    value: json!("1234"),
                },
            ],
            sort: None,
        };

        let query = build_search_query(&request, 10, 0).expect("query builds");
        assert!(query.sql.contains("FROM address_index FINAL"));
        assert!(query.sql.contains("FROM event_field_index FINAL"));
        assert_eq!(query.arguments.len(), 5);
    }
}
