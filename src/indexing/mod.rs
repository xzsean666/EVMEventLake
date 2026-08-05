use serde_json::Value;
use sqlx::QueryBuilder;
use uuid::Uuid;

use crate::shared::{error::ApplicationError, validation::normalize_address};

pub mod partition_manager;

#[derive(Debug, Clone)]
pub struct DecodedFieldValue {
    pub field_name: String,
    pub field_type: String,
    pub normalized_value: String,
    pub json_value: Value,
}

#[derive(Debug, Clone)]
pub struct DecodedEventIndexInput {
    pub chain_id: i64,
    pub contract_address: String,
    pub event_name: String,
    pub raw_log_id: Uuid,
    pub block_number: i64,
    pub transaction_hash: String,
    pub fields: Vec<DecodedFieldValue>,
}

/// Builds the address and event-field indexes for a single decoded event.
///
/// Each index is written as one multi-row insert instead of a query per field, so an
/// event with N fields costs at most two round trips rather than up to 2N.
pub async fn index_decoded_event(
    connection: &mut sqlx::PgConnection,
    input: &DecodedEventIndexInput,
) -> Result<(), ApplicationError> {
    let address_fields: Vec<&DecodedFieldValue> = input
        .fields
        .iter()
        .filter(|field| normalize_address(&field.normalized_value).is_ok())
        .collect();

    if !address_fields.is_empty() {
        let mut builder = QueryBuilder::new(
            r#"
            INSERT INTO eventlake_address_index (
                id, chain_id, address, contract_address, event_name, field_name,
                raw_log_id, block_number, transaction_hash
            )
            "#,
        );
        builder.push_values(address_fields, |mut row, field| {
            row.push_bind(Uuid::new_v4())
                .push_bind(input.chain_id)
                .push_bind(field.normalized_value.clone())
                .push_bind(input.contract_address.clone())
                .push_bind(input.event_name.clone())
                .push_bind(field.field_name.clone())
                .push_bind(input.raw_log_id)
                .push_bind(input.block_number)
                .push_bind(input.transaction_hash.clone());
        });
        builder.push(
            " ON CONFLICT (chain_id, address, raw_log_id, field_name, block_number) DO NOTHING",
        );
        builder.build().execute(&mut *connection).await?;
    }

    let value_fields: Vec<&DecodedFieldValue> = input
        .fields
        .iter()
        .filter(|field| should_index_field_value(&field.json_value))
        .collect();

    if !value_fields.is_empty() {
        let mut builder = QueryBuilder::new(
            r#"
            INSERT INTO eventlake_event_field_index (
                id, chain_id, contract_address, event_name, field_name, field_type,
                normalized_value, raw_log_id, block_number
            )
            "#,
        );
        builder.push_values(value_fields, |mut row, field| {
            row.push_bind(Uuid::new_v4())
                .push_bind(input.chain_id)
                .push_bind(input.contract_address.clone())
                .push_bind(input.event_name.clone())
                .push_bind(field.field_name.clone())
                .push_bind(field.field_type.clone())
                .push_bind(field.normalized_value.clone())
                .push_bind(input.raw_log_id)
                .push_bind(input.block_number);
        });
        builder.push(
            r#"
            ON CONFLICT (
                chain_id, contract_address, event_name, field_name,
                normalized_value, raw_log_id, block_number
            ) DO NOTHING
            "#,
        );
        builder.build().execute(&mut *connection).await?;
    }

    Ok(())
}

/// Compatibility helper for callers that explicitly maintain old decoded-event history.
/// The raw-event-lake collector does not call this path or enqueue decoding work.
#[cfg(feature = "clickhouse")]
pub async fn mirror_decoded_event(
    client: &clickhouse::Client,
    input: crate::clickhouse::IndexedEvent,
) -> anyhow::Result<()> {
    crate::clickhouse::write_indexed_event(client, input).await
}

fn should_index_field_value(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}
