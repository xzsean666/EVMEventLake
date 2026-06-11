use serde_json::Value;
use uuid::Uuid;

use crate::{
    app::application_state::ApplicationState,
    shared::{error::ApplicationError, validation::normalize_address},
};

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

pub async fn index_decoded_event(
    pool: &sqlx::PgPool,
    input: DecodedEventIndexInput,
) -> Result<(), ApplicationError> {
    for field in input.fields {
        if normalize_address(&field.normalized_value).is_ok() {
            sqlx::query(
                r#"
                INSERT INTO address_index (
                    id, chain_id, address, contract_address, event_name, field_name,
                    raw_log_id, block_number, transaction_hash
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (chain_id, address, raw_log_id, field_name, block_number) DO NOTHING
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(input.chain_id)
            .bind(field.normalized_value.clone())
            .bind(&input.contract_address)
            .bind(&input.event_name)
            .bind(&field.field_name)
            .bind(input.raw_log_id)
            .bind(input.block_number)
            .bind(&input.transaction_hash)
            .execute(pool)
            .await?;
        }

        if should_index_field_value(&field.json_value) {
            sqlx::query(
                r#"
                INSERT INTO event_field_index (
                    id, chain_id, contract_address, event_name, field_name, field_type,
                    normalized_value, raw_log_id, block_number
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (
                    chain_id, contract_address, event_name, field_name,
                    normalized_value, raw_log_id, block_number
                ) DO NOTHING
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(input.chain_id)
            .bind(&input.contract_address)
            .bind(&input.event_name)
            .bind(&field.field_name)
            .bind(&field.field_type)
            .bind(&field.normalized_value)
            .bind(input.raw_log_id)
            .bind(input.block_number)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

fn should_index_field_value(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

#[allow(dead_code)]
pub async fn reindex_decoded_event(
    _state: &ApplicationState,
    _raw_log_id: Uuid,
    _block_number: i64,
) -> Result<(), ApplicationError> {
    Err(ApplicationError::Internal(
        "single event reindex is not implemented yet".to_owned(),
    ))
}
