use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::configuration::DatabaseConfiguration;

pub async fn connect(configuration: &DatabaseConfiguration) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(configuration.max_connections)
        .connect(&configuration.database_url)
        .await?;

    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name("eventlake_sqlx_migrations");
    migrator.run(pool).await?;
    Ok(())
}
