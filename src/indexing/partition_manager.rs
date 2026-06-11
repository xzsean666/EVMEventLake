use tokio::time::{MissedTickBehavior, interval};

use crate::{app::application_state::ApplicationState, shared::error::ApplicationError};

const PARTITION_BLOCK_SIZE: i64 = 1_000_000;

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.worker_tick);
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
        create_partition_pair(pool, start).await?;
        create_partition_pair(pool, start + PARTITION_BLOCK_SIZE).await?;
    }

    Ok(())
}

fn floor_partition_start(block_number: i64) -> i64 {
    block_number.div_euclid(PARTITION_BLOCK_SIZE) * PARTITION_BLOCK_SIZE
}

async fn create_partition_pair(pool: &sqlx::PgPool, start: i64) -> Result<(), ApplicationError> {
    let end = start + PARTITION_BLOCK_SIZE;
    let raw_partition = format!("eventlake_raw_logs_{}_{}", start, end);
    let decoded_partition = format!("eventlake_decoded_events_{}_{}", start, end);

    let raw_sql = format!(
        "CREATE TABLE IF NOT EXISTS {raw_partition} PARTITION OF eventlake_raw_logs FOR VALUES FROM ({start}) TO ({end})"
    );
    let decoded_sql = format!(
        "CREATE TABLE IF NOT EXISTS {decoded_partition} PARTITION OF eventlake_decoded_events FOR VALUES FROM ({start}) TO ({end})"
    );

    sqlx::query(sqlx::AssertSqlSafe(raw_sql))
        .execute(pool)
        .await?;
    sqlx::query(sqlx::AssertSqlSafe(decoded_sql))
        .execute(pool)
        .await?;

    Ok(())
}
