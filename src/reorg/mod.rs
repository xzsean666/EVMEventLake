use crate::shared::error::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockCheckpointResult {
    NewBlock,
    SameBlock,
    ReorgDetected {
        previous_hash: String,
        new_hash: String,
    },
}

pub async fn observe_block(
    pool: &sqlx::PgPool,
    chain_id: i64,
    block_number: i64,
    block_hash: &str,
) -> Result<BlockCheckpointResult, ApplicationError> {
    let previous = sqlx::query_as::<_, (String,)>(
        "SELECT block_hash FROM eventlake_block_checkpoints WHERE chain_id = $1 AND block_number = $2",
    )
    .bind(chain_id)
    .bind(block_number)
    .fetch_optional(pool)
    .await?;

    match previous {
        None => {
            sqlx::query(
                r#"
                INSERT INTO eventlake_block_checkpoints (chain_id, block_number, block_hash)
                VALUES ($1, $2, $3)
                ON CONFLICT (chain_id, block_number) DO NOTHING
                "#,
            )
            .bind(chain_id)
            .bind(block_number)
            .bind(block_hash)
            .execute(pool)
            .await?;
            Ok(BlockCheckpointResult::NewBlock)
        }
        Some((previous_hash,)) if previous_hash == block_hash => {
            Ok(BlockCheckpointResult::SameBlock)
        }
        Some((previous_hash,)) => {
            mark_reorg_range_invalid(pool, chain_id, block_number).await?;
            sqlx::query(
                r#"
                UPDATE eventlake_block_checkpoints
                SET block_hash = $3,
                    observed_at = now()
                WHERE chain_id = $1 AND block_number = $2
                "#,
            )
            .bind(chain_id)
            .bind(block_number)
            .bind(block_hash)
            .execute(pool)
            .await?;

            Ok(BlockCheckpointResult::ReorgDetected {
                previous_hash,
                new_hash: block_hash.to_owned(),
            })
        }
    }
}

async fn mark_reorg_range_invalid(
    pool: &sqlx::PgPool,
    chain_id: i64,
    from_block: i64,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        UPDATE eventlake_raw_logs
        SET removed = true
        WHERE chain_id = $1 AND block_number >= $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM eventlake_address_index
        WHERE chain_id = $1 AND block_number >= $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM eventlake_event_field_index
        WHERE chain_id = $1 AND block_number >= $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .execute(pool)
    .await?;

    Ok(())
}
