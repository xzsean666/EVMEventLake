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
    observe_block_with_postgres_search_storage(pool, chain_id, block_number, block_hash, true).await
}

/// Observes one canonical block. PostgreSQL-only deployments invalidate their decoded
/// event/index tables; ClickHouse deployments leave those tables empty and invalidate the
/// ClickHouse projection separately before collection resumes.
pub async fn observe_block_with_postgres_search_storage(
    pool: &sqlx::PgPool,
    chain_id: i64,
    block_number: i64,
    block_hash: &str,
    postgres_search_storage: bool,
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
            // The block hash changed: everything from this block onward on this chain is
            // suspect. Invalidate it and rewind affected subscriptions atomically so the
            // collector re-fetches the canonical fork. Either it all happens or none of it.
            let mut transaction = pool.begin().await?;
            invalidate_from_block(
                &mut transaction,
                chain_id,
                block_number,
                postgres_search_storage,
            )
            .await?;

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
            .execute(&mut *transaction)
            .await?;

            transaction.commit().await?;

            Ok(BlockCheckpointResult::ReorgDetected {
                previous_hash,
                new_hash: block_hash.to_owned(),
            })
        }
    }
}

async fn invalidate_from_block(
    connection: &mut sqlx::PgConnection,
    chain_id: i64,
    from_block: i64,
    postgres_search_storage: bool,
) -> Result<(), ApplicationError> {
    // Raw logs are preserved (per the "keep raw logs permanently" rule) but flagged so
    // collection can re-ingest the canonical fork without violating the unique index.
    sqlx::query(
        r#"
        UPDATE eventlake_raw_logs
        SET removed = true
        WHERE chain_id = $1 AND block_number >= $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .execute(&mut *connection)
    .await?;

    if postgres_search_storage {
        // Decoded events are kept for audit but flipped out of the 'decoded' status so the
        // PostgreSQL search path stops returning reorged data immediately.
        sqlx::query(
            r#"
            UPDATE eventlake_decoded_events
            SET decode_status = 'reorged',
                decode_error = 'invalidated by reorg',
                decoded_at = now()
            WHERE chain_id = $1 AND block_number >= $2 AND decode_status = 'decoded'
            "#,
        )
        .bind(chain_id)
        .bind(from_block)
        .execute(&mut *connection)
        .await?;

        // The derived indexes are rebuilt from scratch on re-decode, so they are deleted.
        sqlx::query(
            r#"
            DELETE FROM eventlake_address_index
            WHERE chain_id = $1 AND block_number >= $2
            "#,
        )
        .bind(chain_id)
        .bind(from_block)
        .execute(&mut *connection)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM eventlake_event_field_index
            WHERE chain_id = $1 AND block_number >= $2
            "#,
        )
        .bind(chain_id)
        .bind(from_block)
        .execute(&mut *connection)
        .await?;

        refresh_contract_registry(connection, chain_id).await?;
    }

    // Rewind subscriptions that had advanced past the reorg point so the collector
    // re-fetches the affected range on its next tick.
    sqlx::query(
        r#"
        UPDATE eventlake_subscriptions
        SET current_block = $2,
            status = 'pending',
            error_message = 'rewound after chain reorg',
            updated_at = now()
        WHERE chain_id = $1 AND active = true AND current_block > $2
        "#,
    )
    .bind(chain_id)
    .bind(from_block)
    .execute(&mut *connection)
    .await?;

    Ok(())
}

async fn refresh_contract_registry(
    connection: &mut sqlx::PgConnection,
    chain_id: i64,
) -> Result<(), ApplicationError> {
    sqlx::query(
        r#"
        WITH stats AS (
            SELECT chain_id,
                   contract_address,
                   COUNT(*)::BIGINT AS event_count,
                   MIN(block_number) AS first_seen_block,
                   MAX(block_number) AS last_seen_block,
                   MIN(decoded_at) AS first_seen_at,
                   MAX(decoded_at) AS last_seen_at
            FROM eventlake_decoded_events
            WHERE chain_id = $1 AND decode_status = 'decoded'
            GROUP BY chain_id, contract_address
        ),
        contracts AS (
            SELECT chain_id, contract_address
            FROM eventlake_contract_registry
            WHERE chain_id = $1
        )
        UPDATE eventlake_contract_registry cr
        SET event_count = COALESCE(stats.event_count, 0),
            first_seen_block = stats.first_seen_block,
            last_seen_block = stats.last_seen_block,
            first_seen_at = stats.first_seen_at,
            last_seen_at = stats.last_seen_at,
            updated_at = now()
        FROM contracts
        LEFT JOIN stats
          ON stats.chain_id = contracts.chain_id
         AND stats.contract_address = contracts.contract_address
        WHERE cr.chain_id = contracts.chain_id
          AND cr.contract_address = contracts.contract_address
        "#,
    )
    .bind(chain_id)
    .execute(&mut *connection)
    .await?;

    Ok(())
}
