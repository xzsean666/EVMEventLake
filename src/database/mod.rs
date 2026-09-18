use std::{str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::configuration::DatabaseConfiguration;

pub async fn connect(configuration: &DatabaseConfiguration) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(&configuration.database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(configuration.max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?;

    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name("eventlake_sqlx_migrations");
    migrator.run(pool).await?;
    Ok(())
}

