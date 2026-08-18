use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::shared::error::ApplicationError;

const SYNC_STATE_COLUMNS: &str = r#"
    chain_id, next_block, start_block, safe_head, latest_seen_block,
    status, realtime_enabled, batch_size, max_concurrency, reorg_window,
    last_error, last_success_at, created_at, updated_at
"#;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema, PartialEq, Eq)]
pub struct BlockTransactionSyncStateRecord {
    pub chain_id: i64,
    pub next_block: i64,
    pub start_block: i64,
    pub safe_head: Option<i64>,
    pub latest_seen_block: Option<i64>,
    pub status: String,
    pub realtime_enabled: bool,
    pub batch_size: i32,
    pub max_concurrency: i32,
    pub reorg_window: i32,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateSyncConfigRequest {
    pub start_block: Option<i64>,
    pub realtime_enabled: Option<bool>,
    pub batch_size: Option<i32>,
    pub max_concurrency: Option<i32>,
    pub reorg_window: Option<i32>,
}

pub async fn get_sync_state(
    pool: &PgPool,
    chain_id: i64,
) -> Result<Option<BlockTransactionSyncStateRecord>, ApplicationError> {
    let query = format!(
        "SELECT {SYNC_STATE_COLUMNS} FROM eventlake_block_transaction_sync_state WHERE chain_id = $1"
    );
    let record = sqlx::query_as::<_, BlockTransactionSyncStateRecord>(sqlx::AssertSqlSafe(query))
        .bind(chain_id)
        .fetch_optional(pool)
        .await?;

    Ok(record)
}

pub async fn runnable_sync_states(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<BlockTransactionSyncStateRecord>, ApplicationError> {
    let query = format!(
        "SELECT {SYNC_STATE_COLUMNS} FROM eventlake_block_transaction_sync_state \
         WHERE status IN ('pending', 'syncing', 'caught_up', 'error', 'reorg_retrying') \
         ORDER BY updated_at ASC LIMIT $1"
    );
    let records = sqlx::query_as::<_, BlockTransactionSyncStateRecord>(sqlx::AssertSqlSafe(query))
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(records)
}

pub async fn upsert_sync_config(
    pool: &PgPool,
    chain_id: i64,
    request: &UpdateSyncConfigRequest,
) -> Result<BlockTransactionSyncStateRecord, ApplicationError> {
    if let Some(start_block) = request.start_block
        && start_block < 0
    {
        return Err(ApplicationError::BadRequest(
            "start_block must be non-negative".to_owned(),
        ));
    }
    if let Some(batch_size) = request.batch_size
        && !(1..=500).contains(&batch_size)
    {
        return Err(ApplicationError::BadRequest(
            "batch_size must be between 1 and 500".to_owned(),
        ));
    }
    if let Some(max_concurrency) = request.max_concurrency
        && !(1..=32).contains(&max_concurrency)
    {
        return Err(ApplicationError::BadRequest(
            "max_concurrency must be between 1 and 32".to_owned(),
        ));
    }
    if let Some(reorg_window) = request.reorg_window
        && !(0..=1024).contains(&reorg_window)
    {
        return Err(ApplicationError::BadRequest(
            "reorg_window must be between 0 and 1024".to_owned(),
        ));
    }

    let default_start_block = request.start_block.unwrap_or(0);
    let default_realtime = request.realtime_enabled.unwrap_or(true);
    let default_batch_size = request.batch_size.unwrap_or(10);
    let default_max_concurrency = request.max_concurrency.unwrap_or(2);
    let default_reorg_window = request.reorg_window.unwrap_or(32);

    let query = format!(
        r#"
        INSERT INTO eventlake_block_transaction_sync_state (
            chain_id, next_block, start_block, status, realtime_enabled,
            batch_size, max_concurrency, reorg_window, updated_at
        )
        VALUES ($1, $2, $3, 'pending', $4, $5, $6, $7, now())
        ON CONFLICT (chain_id) DO UPDATE
        SET start_block = COALESCE($8, eventlake_block_transaction_sync_state.start_block),
            next_block = CASE
                WHEN $8 IS NOT NULL AND eventlake_block_transaction_sync_state.next_block < $8 THEN $8
                ELSE eventlake_block_transaction_sync_state.next_block
            END,
            realtime_enabled = COALESCE($9, eventlake_block_transaction_sync_state.realtime_enabled),
            batch_size = COALESCE($10, eventlake_block_transaction_sync_state.batch_size),
            max_concurrency = COALESCE($11, eventlake_block_transaction_sync_state.max_concurrency),
            reorg_window = COALESCE($12, eventlake_block_transaction_sync_state.reorg_window),
            updated_at = now()
        RETURNING {SYNC_STATE_COLUMNS}
        "#
    );

    let record = sqlx::query_as::<_, BlockTransactionSyncStateRecord>(sqlx::AssertSqlSafe(query))
        .bind(chain_id)
        .bind(default_start_block)
        .bind(default_start_block)
        .bind(default_realtime)
        .bind(default_batch_size)
        .bind(default_max_concurrency)
        .bind(default_reorg_window)
        .bind(request.start_block)
        .bind(request.realtime_enabled)
        .bind(request.batch_size)
        .bind(request.max_concurrency)
        .bind(request.reorg_window)
        .fetch_one(pool)
        .await?;

    Ok(record)
}

pub async fn advance_checkpoint(
    pool: &PgPool,
    chain_id: i64,
    next_block: i64,
    safe_head: Option<i64>,
    latest_seen_block: Option<i64>,
    status: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_block_transaction_sync_state
        SET next_block = $2,
            safe_head = COALESCE($3, safe_head),
            latest_seen_block = COALESCE($4, latest_seen_block),
            status = $5,
            last_error = NULL,
            last_success_at = now(),
            updated_at = now()
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .bind(next_block)
    .bind(safe_head)
    .bind(latest_seen_block)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_sync_error(
    pool: &PgPool,
    chain_id: i64,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_block_transaction_sync_state
        SET status = 'error',
            last_error = $2,
            updated_at = now()
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn pause_sync(
    pool: &PgPool,
    chain_id: i64,
) -> Result<BlockTransactionSyncStateRecord, ApplicationError> {
    let query = format!(
        r#"
        UPDATE eventlake_block_transaction_sync_state
        SET status = 'paused',
            updated_at = now()
        WHERE chain_id = $1
        RETURNING {SYNC_STATE_COLUMNS}
        "#
    );

    let record = sqlx::query_as::<_, BlockTransactionSyncStateRecord>(sqlx::AssertSqlSafe(query))
        .bind(chain_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            ApplicationError::NotFound(format!("sync state for chain {chain_id} not found"))
        })?;

    Ok(record)
}

pub async fn resume_sync(
    pool: &PgPool,
    chain_id: i64,
) -> Result<BlockTransactionSyncStateRecord, ApplicationError> {
    let query = format!(
        r#"
        UPDATE eventlake_block_transaction_sync_state
        SET status = 'syncing',
            updated_at = now()
        WHERE chain_id = $1
        RETURNING {SYNC_STATE_COLUMNS}
        "#
    );

    let record = sqlx::query_as::<_, BlockTransactionSyncStateRecord>(sqlx::AssertSqlSafe(query))
        .bind(chain_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            ApplicationError::NotFound(format!("sync state for chain {chain_id} not found"))
        })?;

    Ok(record)
}

pub async fn rewind_checkpoint_for_reorg(
    pool: &PgPool,
    chain_id: i64,
    rewind_to_block: i64,
    error_message: &str,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_block_transaction_sync_state
        SET next_block = $2,
            status = 'reorg_retrying',
            last_error = $3,
            updated_at = now()
        WHERE chain_id = $1
        "#,
    )
    .bind(chain_id)
    .bind(rewind_to_block)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_sync_config_request_bounds() {
        let valid_request = UpdateSyncConfigRequest {
            start_block: Some(100),
            realtime_enabled: Some(true),
            batch_size: Some(20),
            max_concurrency: Some(4),
            reorg_window: Some(64),
        };
        assert!(valid_request.start_block.unwrap() >= 0);
        assert!((1..=500).contains(&valid_request.batch_size.unwrap()));
        assert!((1..=32).contains(&valid_request.max_concurrency.unwrap()));
        assert!((0..=1024).contains(&valid_request.reorg_window.unwrap()));
    }
}
