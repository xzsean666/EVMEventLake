use std::{env, net::IpAddr, time::Duration};

#[derive(Clone, Debug)]
pub struct ApplicationConfiguration {
    pub http: HttpConfiguration,
    pub database: DatabaseConfiguration,
    pub auth: AuthConfiguration,
    pub background: BackgroundConfiguration,
    pub telemetry: TelemetryConfiguration,
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

        let telemetry = TelemetryConfiguration {
            log_level: read_env("EVENTLAKE_LOG_LEVEL", "info"),
            json_logs: read_env("EVENTLAKE_JSON_LOGS", "false").parse()?,
        };

        Ok(Self {
            http,
            database,
            auth,
            background,
            telemetry,
        })
    }
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
