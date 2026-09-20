use anyhow::Context;
use chrono::{DateTime, Utc};
pub use clickhouse::Client;
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    app::application_state::ApplicationState,
    configuration::ClickHouseConfig,
    search::{
        RawLogRecord, RawLogSearchRequest, SearchFilter, SearchOperator, SearchSort,
    },
    shared::{
        error::ApplicationError,
        validation::{normalize_address, normalize_topic},
    },
};

const SCHEMA: &str = include_str!("../../clickhouse/schema.sql");

pub mod block_transaction;
pub use block_transaction::{
    BlockRow, TransactionRow, get_address_transactions, get_block_by_hash, get_block_by_number,
    get_block_transactions, get_transaction_by_hash, invalidate_blocks_and_transactions_from_block,
    write_blocks_and_transactions,
};

/// An encoded EVM log. ClickHouse raw-event-lake mode writes this directly from the
/// collector and intentionally never requires an ABI or a decoding queue.
#[derive(Clone, Debug)]
pub struct RawLog {
    pub id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: i64,
    pub log_index: i64,
    pub contract_address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub is_removed: bool,
}

#[derive(Row, Serialize)]
struct RawLogRow {
    #[serde(with = "clickhouse::serde::uuid")]
    id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    subscription_id: Option<Uuid>,
    chain_id: u64,
    block_number: u64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u32,
    log_index: u32,
    contract_address: String,
    topic0: String,
    topic1: String,
    topic2: String,
    topic3: String,
    topics: String,
    data: String,
    is_removed: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    ingested_at: OffsetDateTime,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    stored_at: OffsetDateTime,
}

pub async fn connect(configuration: &ClickHouseConfig) -> anyhow::Result<Option<Client>> {
    if !configuration.enabled {
        tracing::info!("ClickHouse disabled");
        return Ok(None);
    }

    let mut client = Client::default().with_url(configuration.url());
    if !configuration.user.is_empty() {
        client = client.with_user(configuration.user.clone());
    }
    if !configuration.password.is_empty() {
        client = client.with_password(configuration.password.clone());
    }
    if !configuration.database.is_empty() {
        client = client.with_database(configuration.database.clone());
    }

    if configuration.async_insert {
        client = client
            .with_option("async_insert", "1")
            .with_option("async_insert_busy_timeout_ms", "200");
        if configuration.wait_for_async_insert {
            client = client.with_option("wait_for_async_insert", "1");
        }
    }
    if !configuration.log_queries {
        client = client.with_option("log_queries", "0");
    }

    client
        .query("SELECT 1")
        .fetch_one::<u8>()
        .await
        .context("ClickHouse healthcheck failed")?;
    initialize_schema(&client).await?;

    tracing::info!(
        url = %configuration.url(),
        host = %configuration.host,
        port = configuration.port,
        database = %configuration.database,
        "ClickHouse connected"
    );
    Ok(Some(client))
}

/// Returns a live analytical client when ClickHouse is enabled. A failed startup
/// connection is retried here so collection can resume without restarting EventLake.
pub async fn active_client(state: &ApplicationState) -> Result<Option<Client>, ApplicationError> {
    if !state.configuration.clickhouse.enabled {
        return Ok(None);
    }

    if let Some(client) = state.clickhouse_client() {
        return Ok(Some(client));
    }

    let client = connect(&state.configuration.clickhouse)
        .await
        .map_err(|error| {
            ApplicationError::ExternalService(format!("ClickHouse unavailable: {error}"))
        })?
        .ok_or_else(|| {
            ApplicationError::ExternalService(
                "ClickHouse is enabled but no client is available".to_owned(),
            )
        })?;
    state.set_clickhouse_client(client.clone());
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

/// Persists an entire RPC response atomically at the collector checkpoint boundary. The
/// caller advances SQLite's subscription checkpoint only after this returns success.
pub async fn write_raw_logs(client: &Client, logs: &[RawLog]) -> anyhow::Result<()> {
    if logs.is_empty() {
        return Ok(());
    }

    let stored_at = Utc::now();
    let stored_at_offset = to_offset_datetime(stored_at)?;
    let rows = logs
        .iter()
        .map(|log| {
            let topics = serde_json::to_string(&log.topics)
                .context("raw-log topics cannot be serialized as JSON")?;
            Ok(RawLogRow {
                id: log.id,
                subscription_id: log.subscription_id,
                chain_id: as_u64(log.chain_id, "chain_id")?,
                block_number: as_u64(log.block_number, "block_number")?,
                block_hash: log.block_hash.clone(),
                transaction_hash: log.transaction_hash.clone(),
                transaction_index: as_u32(log.transaction_index, "transaction_index")?,
                log_index: as_u32(log.log_index, "log_index")?,
                contract_address: log.contract_address.clone(),
                topic0: log.topics.first().cloned().unwrap_or_default(),
                topic1: log.topics.get(1).cloned().unwrap_or_default(),
                topic2: log.topics.get(2).cloned().unwrap_or_default(),
                topic3: log.topics.get(3).cloned().unwrap_or_default(),
                topics,
                data: log.data.clone(),
                is_removed: log.is_removed,
                ingested_at: stored_at_offset,
                stored_at: stored_at_offset,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    write_rows(client, "raw_logs", &rows).await
}

/// Writes raw-log tombstone versions directly from ClickHouse.
pub async fn invalidate_from_block(
    client: &Client,
    chain_id: i64,
    from_block: i64,
) -> anyhow::Result<()> {
    let chain_id = as_u64(chain_id, "chain_id")?;
    let from_block = as_u64(from_block, "from_block")?;

    execute_reorg_tombstone(
        client,
        r#"
        INSERT INTO raw_logs
        SELECT id, subscription_id, chain_id, block_number, block_hash, transaction_hash,
               transaction_index, log_index, contract_address, topic0, topic1, topic2, topic3,
               topics, data, true, ingested_at, now64(3)
        FROM raw_logs FINAL
        WHERE chain_id = ? AND block_number >= ? AND is_removed = false
        "#,
        chain_id,
        from_block,
    )
    .await
}

pub(crate) async fn execute_reorg_tombstone(
    client: &Client,
    statement: &str,
    chain_id: u64,
    from_block: u64,
) -> anyhow::Result<()> {
    client
        .query(statement)
        .bind(chain_id)
        .bind(from_block)
        .execute()
        .await
        .context("failed to write ClickHouse reorg tombstones")
}

pub(crate) async fn write_rows<T>(client: &Client, table: &str, rows: &[T]) -> anyhow::Result<()>
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

pub(crate) fn as_u64(value: i64, field: &str) -> anyhow::Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} cannot be negative"))
}

pub(crate) fn to_offset_datetime(value: DateTime<Utc>) -> anyhow::Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(
        value
            .timestamp_nanos_opt()
            .context("timestamp is outside the supported range")? as i128,
    )
    .context("timestamp cannot be represented by ClickHouse time mapping")
}

pub(crate) fn as_u32(value: i64, field: &str) -> anyhow::Result<u32> {
    u32::try_from(value).with_context(|| format!("{field} is outside ClickHouse UInt32 range"))
}

#[derive(Row, Deserialize)]
struct RawLogSearchRow {
    #[serde(with = "clickhouse::serde::uuid")]
    id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    subscription_id: Option<Uuid>,
    chain_id: u64,
    block_number: u64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u32,
    log_index: u32,
    contract_address: String,
    topics: String,
    data: String,
    is_removed: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    ingested_at: OffsetDateTime,
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

pub async fn search_raw_logs(
    client: &Client,
    request: &RawLogSearchRequest,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<RawLogRecord>> {
    let search = build_raw_log_search_query(request, limit, offset)?;
    let mut query = client.query(&search.sql);
    for argument in search.arguments {
        query = match argument {
            QueryArgument::Text(value) => query.bind(value),
            QueryArgument::Signed(value) => query.bind(value),
            QueryArgument::Unsigned(value) => query.bind(value),
        };
    }

    query
        .fetch_all::<RawLogSearchRow>()
        .await?
        .into_iter()
        .map(|row| {
            Ok(RawLogRecord {
                id: row.id,
                subscription_id: row.subscription_id,
                chain_id: i64::try_from(row.chain_id)
                    .context("ClickHouse chain ID exceeds i64")?,
                block_number: i64::try_from(row.block_number)
                    .context("ClickHouse block number exceeds i64")?,
                block_hash: row.block_hash,
                transaction_hash: row.transaction_hash,
                transaction_index: i64::from(row.transaction_index),
                log_index: i64::from(row.log_index),
                contract_address: row.contract_address,
                topics: serde_json::from_str(&row.topics)
                    .context("invalid raw-log topics JSON in ClickHouse")?,
                data: row.data,
                removed: row.is_removed,
                ingested_at: chrono_from_offset_datetime(row.ingested_at)?,
            })
        })
        .collect()
}

pub async fn raw_log_count(client: &Client) -> anyhow::Result<i64> {
    // Read total active row count from ClickHouse system.parts metadata (zero-disk-scan, O(1)).
    // This completely eliminates cross-partition full-table ReplacingMergeTree FINAL scans,
    // protecting ClickHouse from high CPU/memory spikes and OOM under heavy loads.
    let count = client
        .query("SELECT coalesce(sum(rows), 0) FROM system.parts WHERE table = 'raw_logs' AND active = 1")
        .fetch_one::<u64>()
        .await?;
    i64::try_from(count).context("ClickHouse raw-log count exceeds i64")
}



fn chrono_from_offset_datetime(value: OffsetDateTime) -> anyhow::Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(value.unix_timestamp(), value.nanosecond())
        .context("ClickHouse timestamp cannot be converted to chrono")
}



fn build_raw_log_search_query(
    request: &RawLogSearchRequest,
    limit: i64,
    offset: i64,
) -> Result<SearchQuery, ApplicationError> {
    let mut query = SearchQuery {
        sql: String::from(
            r#"
            SELECT id, subscription_id, chain_id, block_number, block_hash, transaction_hash,
                   transaction_index, log_index, contract_address, topics, data, is_removed,
                   ingested_at
            FROM raw_logs FINAL
            WHERE is_removed = false
            "#,
        ),
        arguments: Vec::new(),
    };

    for filter in &request.filters {
        push_raw_log_search_filter(&mut query, filter)?;
    }

    push_raw_log_search_sort(&mut query, request.sort.as_ref())?;
    query.sql.push_str(" LIMIT ? OFFSET ?");
    query
        .arguments
        .push(QueryArgument::Unsigned(u64::try_from(limit).map_err(
            |_| ApplicationError::BadRequest("raw-log search limit cannot be negative".to_owned()),
        )?));
    query
        .arguments
        .push(QueryArgument::Unsigned(u64::try_from(offset).map_err(
            |_| ApplicationError::BadRequest("raw-log search offset cannot be negative".to_owned()),
        )?));

    Ok(query)
}

fn push_raw_log_search_filter(
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
        "transaction_hash" => {
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
        "topic0" | "topic1" | "topic2" | "topic3" => {
            if !matches!(filter.operator, SearchOperator::Eq) {
                return Err(ApplicationError::BadRequest(format!(
                    "{} currently supports eq",
                    filter.field
                )));
            }
            let topic = normalize_topic(&string_value(&filter.value)?)?;
            query.sql.push_str(" AND ");
            query.sql.push_str(filter.field.as_str());
            query.sql.push_str(" = ?");
            query.arguments.push(QueryArgument::Text(topic));
            Ok(())
        }
        _ => Err(ApplicationError::BadRequest(
            "invalid raw-log filter".to_owned(),
        )),
    }
}

fn push_raw_log_search_sort(
    query: &mut SearchQuery,
    sort: Option<&SearchSort>,
) -> Result<(), ApplicationError> {
    match sort {
        Some(sort) if sort.field == "block_number" => {
            query.sql.push_str(" ORDER BY block_number ");
            push_clickhouse_direction(query, sort.direction.as_deref());
            query.sql.push_str(", log_index ");
            push_clickhouse_direction(query, sort.direction.as_deref());
        }
        Some(sort) if sort.field == "log_index" => {
            query.sql.push_str(" ORDER BY log_index ");
            push_clickhouse_direction(query, sort.direction.as_deref());
        }
        Some(sort) if sort.field == "ingested_at" => {
            query.sql.push_str(" ORDER BY ingested_at ");
            push_clickhouse_direction(query, sort.direction.as_deref());
        }
        Some(sort) => {
            return Err(ApplicationError::BadRequest(format!(
                "unsupported raw-log sort field: {}",
                sort.field
            )));
        }
        None => {
            query
                .sql
                .push_str(" ORDER BY block_number DESC, log_index DESC");
        }
    }
    Ok(())
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

fn push_clickhouse_direction(query: &mut SearchQuery, direction: Option<&str>) {
    query
        .sql
        .push_str(if matches!(direction, Some("asc") | Some("ASC")) {
            "ASC"
        } else {
            "DESC"
        });
}

fn string_value(value: &Value) -> Result<String, ApplicationError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ApplicationError::BadRequest("filter value must be a string".to_owned()))
}
