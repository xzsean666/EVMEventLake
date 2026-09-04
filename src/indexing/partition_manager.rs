use std::{
    collections::HashSet,
    sync::{LazyLock, RwLock},
};
use tokio::time::{MissedTickBehavior, interval};

use crate::{app::application_state::ApplicationState, shared::error::ApplicationError};

const PARTITION_BLOCK_SIZE: i64 = 1_000_000;

static CREATED_PARTITIONS: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.partition_tick);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(error) = ensure_partitions(&state.pool).await {
            tracing::warn!(error = %error, "partition manager tick failed");
        }
    }
}

pub async fn ensure_partitions(pool: &sqlx::PgPool) -> Result<(), ApplicationError> {
    let block_numbers = sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT current_block FROM eventlake_subscriptions WHERE active = true
        UNION
        SELECT COALESCE(MAX(block_number), 0) FROM eventlake_raw_logs
        "#,
    )
    .fetch_all(pool)
    .await?;

    for (block_number,) in block_numbers {
        let start = floor_partition_start(block_number);
        create_raw_partition(pool, start).await?;
        create_raw_partition(pool, start + PARTITION_BLOCK_SIZE).await?;
        create_decoded_partition(pool, start).await?;
        create_decoded_partition(pool, start + PARTITION_BLOCK_SIZE).await?;
    }

    Ok(())
}

pub async fn ensure_partitions_for_range(
    pool: &sqlx::PgPool,
    from_block: i64,
    to_block: i64,
) -> Result<(), ApplicationError> {
    for start in partition_starts(from_block, to_block)? {
        create_raw_partition(pool, start).await?;
    }

    Ok(())
}

pub async fn ensure_decoded_partitions_for_range(
    pool: &sqlx::PgPool,
    from_block: i64,
    to_block: i64,
) -> Result<(), ApplicationError> {
    for start in partition_starts(from_block, to_block)? {
        create_decoded_partition(pool, start).await?;
    }

    Ok(())
}

fn partition_starts(from_block: i64, to_block: i64) -> Result<Vec<i64>, ApplicationError> {
    if from_block < 0 || to_block < from_block {
        return Err(ApplicationError::BadRequest(
            "partition range must be non-negative and ordered".to_owned(),
        ));
    }

    let end_start = floor_partition_start(to_block);
    let mut start = floor_partition_start(from_block);
    let mut starts = Vec::new();
    loop {
        starts.push(start);
        if start >= end_start {
            break;
        }
        start += PARTITION_BLOCK_SIZE;
    }

    Ok(starts)
}

fn floor_partition_start(block_number: i64) -> i64 {
    block_number.div_euclid(PARTITION_BLOCK_SIZE) * PARTITION_BLOCK_SIZE
}

async fn create_raw_partition(pool: &sqlx::PgPool, start: i64) -> Result<(), ApplicationError> {
    let end = start + PARTITION_BLOCK_SIZE;
    let raw_partition = format!("eventlake_raw_logs_{}_{}", start, end);
    {
        let cache = CREATED_PARTITIONS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.contains(&raw_partition) {
            return Ok(());
        }
    }

    let raw_sql = format!(
        "CREATE TABLE IF NOT EXISTS {raw_partition} PARTITION OF eventlake_raw_logs FOR VALUES FROM ({start}) TO ({end})"
    );

    sqlx::query(sqlx::AssertSqlSafe(raw_sql))
        .execute(pool)
        .await?;

    let mut cache = CREATED_PARTITIONS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(raw_partition);

    Ok(())
}

async fn create_decoded_partition(pool: &sqlx::PgPool, start: i64) -> Result<(), ApplicationError> {
    let end = start + PARTITION_BLOCK_SIZE;
    let decoded_partition = format!("eventlake_decoded_events_{}_{}", start, end);
    {
        let cache = CREATED_PARTITIONS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.contains(&decoded_partition) {
            return Ok(());
        }
    }

    let decoded_sql = format!(
        "CREATE TABLE IF NOT EXISTS {decoded_partition} PARTITION OF eventlake_decoded_events FOR VALUES FROM ({start}) TO ({end})"
    );

    sqlx::query(sqlx::AssertSqlSafe(decoded_sql))
        .execute(pool)
        .await?;

    let mut cache = CREATED_PARTITIONS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(decoded_partition);

    Ok(())
}
