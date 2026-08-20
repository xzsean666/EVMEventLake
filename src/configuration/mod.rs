use std::{env, net::IpAddr, time::Duration};

#[derive(Clone, Debug)]
pub struct ApplicationConfiguration {
    pub http: HttpConfiguration,
    pub database: DatabaseConfiguration,
    pub clickhouse: ClickHouseConfig,
    pub auth: AuthConfiguration,
    pub background: BackgroundConfiguration,
    pub block_transaction: BlockTransactionConfiguration,
    pub rpc_pool: RpcPoolConfiguration,
    pub telemetry: TelemetryConfiguration,
}

#[derive(Clone, Debug, Default)]
pub struct RpcPoolConfiguration {
    pub seeds_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BlockTransactionConfiguration {
    pub enabled: bool,
    pub batch_size: i32,
    pub max_concurrency: i32,
    pub reorg_window: i32,
    pub max_response_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct HttpConfiguration {
    pub host: IpAddr,
    pub port: u16,
    /// Allowed CORS origins. Empty means "permissive" (any origin) for local development.
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DatabaseConfiguration {
    pub database_url: String,
    pub max_connections: u32,
}

#[derive(Clone, Debug)]
pub struct ClickHouseConfig {
    pub host: String,
    /// HTTP port. The Rust client uses ClickHouse's HTTP interface, not its native protocol.
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    pub enabled: bool,
}

impl ClickHouseConfig {
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

#[derive(Clone, Debug)]
pub struct AuthConfiguration {
    pub jwt_secret: String,
    pub require_authentication: bool,
}

#[derive(Clone, Debug)]
pub struct BackgroundConfiguration {
    pub workers_enabled: bool,
    pub worker_tick: Duration,
    pub decode_batch_size: i64,
    /// Partition maintenance issues DDL, so it runs on a slower cadence than the
    /// collect/decode workers instead of on every worker tick.
    pub partition_tick: Duration,
}

#[derive(Clone, Debug)]
pub struct TelemetryConfiguration {
    pub log_level: String,
    pub json_logs: bool,
}

impl ApplicationConfiguration {
    pub fn from_environment() -> anyhow::Result<Self> {
        let http = HttpConfiguration {
            host: read_env("EVENTLAKE_HTTP_HOST", "127.0.0.1").parse()?,
            port: read_env("EVENTLAKE_HTTP_PORT", "8080").parse()?,
            cors_allowed_origins: parse_comma_separated(&read_env(
                "EVENTLAKE_CORS_ALLOWED_ORIGINS",
                "",
            )),
        };

        let database = DatabaseConfiguration {
            database_url: read_env(
                "EVENTLAKE_DATABASE_URL",
                "postgres://eventlake:eventlake@localhost:5432/eventlake",
            ),
            max_connections: read_positive_u32_env("EVENTLAKE_DATABASE_MAX_CONNECTIONS", "10")?,
        };

        let clickhouse = ClickHouseConfig {
            host: read_env("EVENTLAKE_CLICKHOUSE_HOST", "localhost"),
            port: read_positive_u16_env("EVENTLAKE_CLICKHOUSE_PORT", "8123")?,
            user: read_env("EVENTLAKE_CLICKHOUSE_USER", "eventlake"),
            password: read_env("EVENTLAKE_CLICKHOUSE_PASSWORD", "eventlake"),
            database: read_env("EVENTLAKE_CLICKHOUSE_DB", "eventlake"),
            // The feature alone never changes the existing PostgreSQL-only deployment.
            enabled: read_env("EVENTLAKE_CLICKHOUSE_ENABLED", "false").parse()?,
        };

        let auth = AuthConfiguration {
            jwt_secret: read_env("EVENTLAKE_JWT_SECRET", "change-me"),
            require_authentication: read_env("EVENTLAKE_REQUIRE_AUTHENTICATION", "false")
                .parse()?,
        };

        let background = BackgroundConfiguration {
            workers_enabled: read_env("EVENTLAKE_BACKGROUND_WORKERS_ENABLED", "true").parse()?,
            worker_tick: Duration::from_secs(read_positive_u64_env(
                "EVENTLAKE_WORKER_TICK_SECONDS",
                "5",
            )?),
            decode_batch_size: read_positive_i64_env("EVENTLAKE_DECODE_BATCH_SIZE", "100")?,
            partition_tick: Duration::from_secs(read_positive_u64_env(
                "EVENTLAKE_PARTITION_TICK_SECONDS",
                "300",
            )?),
        };

        let block_transaction = BlockTransactionConfiguration {
            enabled: read_env("EVENTLAKE_BLOCK_TRANSACTION_ENABLED", "false").parse()?,
            batch_size: read_positive_i32_env("EVENTLAKE_BLOCK_TRANSACTION_BATCH_SIZE", "10")?,
            max_concurrency: read_positive_i32_env(
                "EVENTLAKE_BLOCK_TRANSACTION_MAX_CONCURRENCY",
                "2",
            )?,
            reorg_window: read_non_negative_i32_env(
                "EVENTLAKE_BLOCK_TRANSACTION_REORG_WINDOW",
                "32",
            )?,
            max_response_bytes: read_positive_usize_env(
                "EVENTLAKE_BLOCK_TRANSACTION_MAX_RESPONSE_BYTES",
                "67108864",
            )?,
        };

        let rpc_seeds_path = env::var("EVENTLAKE_RPC_SEEDS_PATH")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let default_path = "config/rpc_endpoints.json";
                if std::path::Path::new(default_path).is_file() {
                    Some(default_path.to_owned())
                } else {
                    None
                }
            });

        let rpc_pool = RpcPoolConfiguration {
            seeds_path: rpc_seeds_path,
        };

        let telemetry = TelemetryConfiguration {
            log_level: read_env("EVENTLAKE_LOG_LEVEL", "info"),
            json_logs: read_env("EVENTLAKE_JSON_LOGS", "false").parse()?,
        };

        Ok(Self {
            http,
            database,
            clickhouse,
            auth,
            background,
            block_transaction,
            rpc_pool,
            telemetry,
        })
    }
}

fn read_positive_i32_env(name: &str, default_value: &str) -> anyhow::Result<i32> {
    let value = read_env(name, default_value).parse()?;
    if value < 1 {
        anyhow::bail!("{name} must be at least 1");
    }

    Ok(value)
}

fn read_non_negative_i32_env(name: &str, default_value: &str) -> anyhow::Result<i32> {
    let value = read_env(name, default_value).parse()?;
    if value < 0 {
        anyhow::bail!("{name} must be non-negative");
    }

    Ok(value)
}

fn read_positive_usize_env(name: &str, default_value: &str) -> anyhow::Result<usize> {
    let value = read_env(name, default_value).parse()?;
    if value == 0 {
        anyhow::bail!("{name} must be greater than 0");
    }

    Ok(value)
}

fn read_env(name: &str, default_value: &str) -> String {
    env::var(name).unwrap_or_else(|_| default_value.to_owned())
}

fn read_positive_u32_env(name: &str, default_value: &str) -> anyhow::Result<u32> {
    let value = read_env(name, default_value).parse()?;
    if value == 0 {
        anyhow::bail!("{name} must be greater than 0");
    }

    Ok(value)
}

fn read_positive_u16_env(name: &str, default_value: &str) -> anyhow::Result<u16> {
    let value = read_env(name, default_value).parse()?;
    if value == 0 {
        anyhow::bail!("{name} must be greater than 0");
    }

    Ok(value)
}

fn read_positive_u64_env(name: &str, default_value: &str) -> anyhow::Result<u64> {
    let value = read_env(name, default_value).parse()?;
    if value == 0 {
        anyhow::bail!("{name} must be greater than 0");
    }

    Ok(value)
}

fn read_positive_i64_env(name: &str, default_value: &str) -> anyhow::Result<i64> {
    let value = read_env(name, default_value).parse()?;
    if value < 1 {
        anyhow::bail!("{name} must be at least 1");
    }

    Ok(value)
}

fn parse_comma_separated(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ClickHouseConfig;

    #[test]
    fn clickhouse_url_targets_the_http_endpoint() {
        let configuration = ClickHouseConfig {
            host: "clickhouse".to_owned(),
            port: 8123,
            user: "eventlake".to_owned(),
            password: "eventlake".to_owned(),
            database: "eventlake".to_owned(),
            enabled: true,
        };

        assert_eq!(configuration.url(), "http://clickhouse:8123");
    }
}
