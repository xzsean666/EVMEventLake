use std::str::FromStr;

use alloy_dyn_abi::{DynSolValue, EventExt};
use alloy_json_abi::Event;
use alloy_primitives::B256;
use serde_json::{Map, Value, json};
use sqlx::FromRow;
use tokio::time::{MissedTickBehavior, interval};
use uuid::Uuid;

use crate::{
    abi_registry,
    app::application_state::ApplicationState,
    indexing::{self, DecodedEventIndexInput, DecodedFieldValue},
    shared::{error::ApplicationError, validation::normalize_topic},
};

const MAX_DECODE_ATTEMPTS: i32 = 5;

#[derive(Debug, FromRow)]
struct DecodeWorkItem {
    queue_id: Uuid,
    raw_log_id: Uuid,
    block_number: i64,
    chain_id: i64,
    contract_address: String,
    transaction_hash: String,
    topics: Value,
    data: String,
    abi_id: Option<Uuid>,
}

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.worker_tick);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(error) = decode_once(&state).await {
            tracing::warn!(error = %error, "decoder worker tick failed");
        }
    }
}

pub async fn decode_once(state: &ApplicationState) -> Result<(), ApplicationError> {
    let batch = fetch_decode_batch(state).await?;

    for item in batch {
        // The success path marks the queue entry inside its own transaction, so here we
        // only need to record failures.
        if let Err(error) = decode_work_item(state, &item).await {
            mark_queue_status(
                &state.pool,
                item.queue_id,
                "error",
                Some(&error.public_message()),
            )
            .await?;
        }
    }

    Ok(())
}

async fn fetch_decode_batch(
    state: &ApplicationState,
) -> Result<Vec<DecodeWorkItem>, ApplicationError> {
    let batch = sqlx::query_as::<_, DecodeWorkItem>(
        r#"
        SELECT dq.id AS queue_id,
               dq.raw_log_id,
               dq.block_number,
               rl.chain_id,
               rl.contract_address,
               rl.transaction_hash,
               rl.topics,
               rl.data,
               s.abi_id
        FROM eventlake_decode_queue dq
        JOIN eventlake_raw_logs rl ON rl.id = dq.raw_log_id AND rl.block_number = dq.block_number
        LEFT JOIN eventlake_subscriptions s ON s.id = dq.subscription_id
        WHERE dq.status IN ('pending', 'error')
          AND rl.removed = false
          AND dq.attempt_count < $2
        ORDER BY dq.created_at
        LIMIT $1
        "#,
    )
    .bind(state.configuration.background.decode_batch_size)
    .bind(MAX_DECODE_ATTEMPTS)
    .fetch_all(&state.pool)
    .await?;

    Ok(batch)
}

async fn decode_work_item(
    state: &ApplicationState,
    item: &DecodeWorkItem,
) -> Result<(), ApplicationError> {
    let abi_id = item
        .abi_id
        .ok_or_else(|| ApplicationError::BadRequest("raw log has no ABI association".to_owned()))?;
    let topic0 = topic0_from_value(&item.topics)?;

    // Pull the parsed ABI from the in-memory cache instead of re-reading and re-parsing
    // the ABI JSON from the database on every single log.
    let cached_abi = abi_registry::load_cached_abi(state, abi_id).await?;
    let event = cached_abi
        .event_for_topic0(&topic0)
        .ok_or_else(|| ApplicationError::BadRequest(format!("no ABI event for topic0 {topic0}")))?;
    let event_name = event.name.clone();

    let topics = parse_topics(&item.topics)?;
    let data = parse_data(&item.data)?;
    let decoded = event
        .decode_log_parts(topics, &data)
        .map_err(|error| ApplicationError::BadRequest(format!("decode failed: {error}")))?;

    let decoded_fields = decoded_event_fields(event, decoded.indexed, decoded.body)?;
    let decoded_event_id = Uuid::new_v4();
    indexing::partition_manager::ensure_decoded_partitions_for_range(
        &state.pool,
        item.block_number,
        item.block_number,
    )
    .await?;

    // The decoded event, its derived indexes, the contract activity bump, and the queue
    // status are one logical unit and are committed together.
    let mut transaction = state.pool.begin().await?;

    // Partitioned tables cannot return system columns (e.g. xmax), so we detect a fresh
    // insert with DO NOTHING + RETURNING. On a real insert the row is complete; on a
    // re-decode (conflict) we refresh the decoded fields with an explicit UPDATE. The
    // contract event_count is bumped only on first insert, avoiding drift on re-decode.
    let inserted = sqlx::query_as::<_, (Uuid,)>(
        r#"
        INSERT INTO eventlake_decoded_events (
            id, raw_log_id, block_number, chain_id, contract_address, abi_id, event_name,
            topic0, indexed_fields, non_indexed_fields, decode_status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'decoded')
        ON CONFLICT (raw_log_id, block_number) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(decoded_event_id)
    .bind(item.raw_log_id)
    .bind(item.block_number)
    .bind(item.chain_id)
    .bind(&item.contract_address)
    .bind(abi_id)
    .bind(&event_name)
    .bind(&topic0)
    .bind(&decoded_fields.indexed_json)
    .bind(&decoded_fields.non_indexed_json)
    .fetch_optional(&mut *transaction)
    .await?
    .is_some();

    let was_decoded_before_update = if inserted {
        false
    } else {
        sqlx::query_as::<_, (String,)>(
            r#"
            SELECT decode_status
            FROM eventlake_decoded_events
            WHERE raw_log_id = $1 AND block_number = $2
            FOR UPDATE
            "#,
        )
        .bind(item.raw_log_id)
        .bind(item.block_number)
        .fetch_optional(&mut *transaction)
        .await?
        .is_some_and(|(status,)| status == "decoded")
    };

    if !inserted {
        sqlx::query(
            r#"
            UPDATE eventlake_decoded_events
            SET abi_id = $3,
                event_name = $4,
                topic0 = $5,
                indexed_fields = $6,
                non_indexed_fields = $7,
                decode_status = 'decoded',
                decode_error = NULL,
                decoded_at = now()
            WHERE raw_log_id = $1 AND block_number = $2
            "#,
        )
        .bind(item.raw_log_id)
        .bind(item.block_number)
        .bind(abi_id)
        .bind(&event_name)
        .bind(&topic0)
        .bind(&decoded_fields.indexed_json)
        .bind(&decoded_fields.non_indexed_json)
        .execute(&mut *transaction)
        .await?;
    }

    indexing::index_decoded_event(
        &mut transaction,
        DecodedEventIndexInput {
            chain_id: item.chain_id,
            contract_address: item.contract_address.clone(),
            event_name,
            raw_log_id: item.raw_log_id,
            block_number: item.block_number,
            transaction_hash: item.transaction_hash.clone(),
            fields: decoded_fields.all_fields,
        },
    )
    .await?;

    if inserted || !was_decoded_before_update {
        update_contract_activity(
            &mut transaction,
            item.chain_id,
            &item.contract_address,
            item.block_number,
        )
        .await?;
    }

    mark_queue_status(&mut *transaction, item.queue_id, "decoded", None).await?;

    transaction.commit().await?;

    Ok(())
}

async fn mark_queue_status<'executor, E>(
    executor: E,
    queue_id: Uuid,
    status: &str,
    last_error: Option<&str>,
) -> Result<(), ApplicationError>
where
    E: sqlx::Executor<'executor, Database = sqlx::Postgres>,
{
    sqlx::query(
        r#"
        UPDATE eventlake_decode_queue
        SET status = $2,
            attempt_count = CASE WHEN $2 = 'error' THEN attempt_count + 1 ELSE attempt_count END,
            last_error = $3,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(queue_id)
    .bind(status)
    .bind(last_error)
    .execute(executor)
    .await?;

    Ok(())
}

async fn update_contract_activity(
    connection: &mut sqlx::PgConnection,
    chain_id: i64,
    contract_address: &str,
    block_number: i64,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_contract_registry
        SET event_count = event_count + 1,
            first_seen_block = COALESCE(first_seen_block, $3),
            last_seen_block = GREATEST(COALESCE(last_seen_block, $3), $3),
            first_seen_at = COALESCE(first_seen_at, now()),
            last_seen_at = now(),
            updated_at = now()
        WHERE chain_id = $1 AND contract_address = $2
        "#,
    )
    .bind(chain_id)
    .bind(contract_address)
    .bind(block_number)
    .execute(&mut *connection)
    .await?;

    Ok(())
}

fn topic0_from_value(topics: &Value) -> Result<String, ApplicationError> {
    topics
        .as_array()
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(normalize_topic)
        .transpose()?
        .ok_or_else(|| ApplicationError::BadRequest("raw log is missing topic0".to_owned()))
}

fn parse_topics(topics: &Value) -> Result<Vec<B256>, ApplicationError> {
    let values = topics
        .as_array()
        .ok_or_else(|| ApplicationError::BadRequest("topics must be an array".to_owned()))?;

    values
        .iter()
        .map(|value| {
            let topic = value
                .as_str()
                .ok_or_else(|| ApplicationError::BadRequest("topic must be a string".to_owned()))?;
            B256::from_str(topic)
                .map_err(|_| ApplicationError::BadRequest(format!("invalid topic: {topic}")))
        })
        .collect()
}

fn parse_data(data: &str) -> Result<Vec<u8>, ApplicationError> {
    let trimmed = data.strip_prefix("0x").unwrap_or(data);
    hex::decode(trimmed)
        .map_err(|error| ApplicationError::BadRequest(format!("invalid log data hex: {error}")))
}

struct DecodedFields {
    indexed_json: Value,
    non_indexed_json: Value,
    all_fields: Vec<DecodedFieldValue>,
}

fn decoded_event_fields(
    event: &Event,
    indexed_values: Vec<DynSolValue>,
    body_values: Vec<DynSolValue>,
) -> Result<DecodedFields, ApplicationError> {
    let indexed_params = event
        .inputs
        .iter()
        .filter(|input| input.indexed)
        .collect::<Vec<_>>();
    let body_params = event
        .inputs
        .iter()
        .filter(|input| !input.indexed)
        .collect::<Vec<_>>();

    let mut indexed_json = Map::new();
    let mut non_indexed_json = Map::new();
    let mut all_fields = Vec::new();

    for (index, value) in indexed_values.into_iter().enumerate() {
        let parameter = indexed_params.get(index);
        let field_name = parameter
            .map(|parameter| parameter.name.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("indexed_arg_{index}"));
        let field_type = parameter
            .map(|parameter| parameter.ty.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        let json_value = dyn_value_to_json(&value);
        indexed_json.insert(field_name.clone(), json_value.clone());
        all_fields.push(DecodedFieldValue {
            field_name,
            field_type,
            normalized_value: normalize_dyn_value(&value),
            json_value,
        });
    }

    for (index, value) in body_values.into_iter().enumerate() {
        let parameter = body_params.get(index);
        let field_name = parameter
            .map(|parameter| parameter.name.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("arg_{index}"));
        let field_type = parameter
            .map(|parameter| parameter.ty.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        let json_value = dyn_value_to_json(&value);
        non_indexed_json.insert(field_name.clone(), json_value.clone());
        all_fields.push(DecodedFieldValue {
            field_name,
            field_type,
            normalized_value: normalize_dyn_value(&value),
            json_value,
        });
    }

    Ok(DecodedFields {
        indexed_json: Value::Object(indexed_json),
        non_indexed_json: Value::Object(non_indexed_json),
        all_fields,
    })
}

fn dyn_value_to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Bool(value) => json!(value),
        DynSolValue::Int(value, _) => json!(value.to_string()),
        DynSolValue::Uint(value, _) => json!(value.to_string()),
        DynSolValue::FixedBytes(value, size) => {
            json!(format!("0x{}", hex::encode(&value[..*size])))
        }
        DynSolValue::Address(value) => json!(format!("{:#x}", value).to_ascii_lowercase()),
        DynSolValue::Function(value) => json!(format!("{value:?}")),
        DynSolValue::Bytes(value) => json!(format!("0x{}", hex::encode(value))),
        DynSolValue::String(value) => json!(value),
        DynSolValue::Array(values)
        | DynSolValue::FixedArray(values)
        | DynSolValue::Tuple(values) => {
            Value::Array(values.iter().map(dyn_value_to_json).collect())
        }
    }
}

fn normalize_dyn_value(value: &DynSolValue) -> String {
    match dyn_value_to_json(value) {
        Value::String(value) => value.to_ascii_lowercase(),
        other => other.to_string().to_ascii_lowercase(),
    }
}
