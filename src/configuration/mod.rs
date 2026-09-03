use std::{env, net::IpAddr, time::Duration};

use anyhow::Context;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickHouseConfig {
    pub host: String,
    /// HTTP port. The Rust client uses ClickHouse's HTTP interface, not its native protocol.
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    pub enabled: bool,
    pub secure: bool,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_owned(),
            port: 8123,
            user: "eventlake".to_owned(),
            password: "eventlake".to_owned(),
            database: "eventlake".to_owned(),
            enabled: false,
            secure: false,
        }
    }
}

impl ClickHouseConfig {
    pub fn url(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    /// Parses a ClickHouse connection URL.
    ///
    /// Supported URL schemes:
    /// - `http://`
    /// - `https://`
    /// - `clickhouse://` (treated as http, default port 8123)
    /// - `clickhouses://` (treated as https, default port 8443)
    ///
    /// Supported format:
    /// `[scheme]://[user[:password]@]host[:port][/database]`
    pub fn from_url(raw_url: &str) -> anyhow::Result<Self> {
        let raw_url = raw_url.trim();
        if raw_url.is_empty() {
            anyhow::bail!("ClickHouse URL cannot be empty");
        }

        let mut parsed_url_str = raw_url.to_owned();
        let mut clickhouse_scheme = false;
        let mut secure = false;

        if let Some(rest) = raw_url.strip_prefix("clickhouses://") {
            parsed_url_str = format!("https://{rest}");
            clickhouse_scheme = true;
            secure = true;
        } else if let Some(rest) = raw_url.strip_prefix("clickhouse://") {
            parsed_url_str = format!("http://{rest}");
            clickhouse_scheme = true;
        }

        let parsed = url::Url::parse(&parsed_url_str)
            .with_context(|| format!("invalid ClickHouse URL: {raw_url}"))?;

        match parsed.scheme() {
            "http" => {
                if !clickhouse_scheme {
                    secure = false;
                }
            }
            "https" => {
                secure = true;
            }
            other => {
                anyhow::bail!(
                    "unsupported scheme '{other}' in ClickHouse URL. Use http, https, clickhouse, or clickhouses"
                );
            }
        }

        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("ClickHouse URL missing host: {raw_url}"))?
            .to_owned();

        let default_port = if secure { 8443 } else { 8123 };
        let port = parsed.port().unwrap_or(default_port);

        let user = if !parsed.username().is_empty() {
            percent_encoding::percent_decode_str(parsed.username())
                .decode_utf8()
                .with_context(|| "ClickHouse URL username contains invalid UTF-8")?
                .into_owned()
        } else {
            "default".to_owned()
        };

        let password = if let Some(pw) = parsed.password() {
            percent_encoding::percent_decode_str(pw)
                .decode_utf8()
                .with_context(|| "ClickHouse URL password contains invalid UTF-8")?
                .into_owned()
        } else {
            String::new()
        };

        let path = parsed.path().trim_start_matches('/');
        let database = if !path.is_empty() {
            percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .with_context(|| "ClickHouse URL database contains invalid UTF-8")?
                .into_owned()
        } else {
            "default".to_owned()
        };

        Ok(Self {
            host,
            port,
            user,
            password,
            database,
            enabled: true,
            secure,
        })
    }

    /// Reads ClickHouse configuration from the environment.
    ///
    /// Priority:
    /// 1. If `EVENTLAKE_CLICKHOUSE_URL` is set, parses it.
    /// 2. If individual variables (`EVENTLAKE_CLICKHOUSE_HOST`, etc.) are also set, they override the URL values.
    /// 3. If `EVENTLAKE_CLICKHOUSE_URL` is not set, reads the individual variables.
    /// 4. `enabled` defaults to `true` if `EVENTLAKE_CLICKHOUSE_URL` is present, or `false` if omitted;
    ///    explicit `EVENTLAKE_CLICKHOUSE_ENABLED` always takes precedence.
    pub fn from_environment() -> anyhow::Result<Self> {
        let url_var = env::var("EVENTLAKE_CLICKHOUSE_URL").ok();
        let url_val = url_var.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let mut config = if let Some(url) = url_val {
            Self::from_url(url)?
        } else {
            Self {
                host: read_env("EVENTLAKE_CLICKHOUSE_HOST", "localhost"),
                port: read_positive_u16_env("EVENTLAKE_CLICKHOUSE_PORT", "8123")?,
                user: read_env("EVENTLAKE_CLICKHOUSE_USER", "eventlake"),
                password: read_env("EVENTLAKE_CLICKHOUSE_PASSWORD", "eventlake"),
                database: read_env("EVENTLAKE_CLICKHOUSE_DB", "eventlake"),
                enabled: false,
                secure: false,
            }
        };

        if let Ok(enabled_str) = env::var("EVENTLAKE_CLICKHOUSE_ENABLED") {
            config.enabled = enabled_str.trim().parse().with_context(|| {
                format!("invalid boolean value for EVENTLAKE_CLICKHOUSE_ENABLED: {enabled_str}")
            })?;
        } else if url_val.is_none() {
            config.enabled = false;
        } else {
            config.enabled = true;
        }

        if let Ok(host) = env::var("EVENTLAKE_CLICKHOUSE_HOST") {
            let host = host.trim();
            if !host.is_empty() {
                config.host = host.to_owned();
            }
        }
        if let Ok(port) = env::var("EVENTLAKE_CLICKHOUSE_PORT") {
            let port = port.trim();
            if !port.is_empty() {
                config.port = port.parse().with_context(|| {
                    format!("invalid port value for EVENTLAKE_CLICKHOUSE_PORT: {port}")
                })?;
            }
        }
        if let Ok(user) = env::var("EVENTLAKE_CLICKHOUSE_USER") {
            let user = user.trim();
            if !user.is_empty() {
                config.user = user.to_owned();
            }
        }
        if let Ok(password) = env::var("EVENTLAKE_CLICKHOUSE_PASSWORD") {
            config.password = password;
        }
        if let Ok(database) = env::var("EVENTLAKE_CLICKHOUSE_DB") {
            let database = database.trim();
            if !database.is_empty() {
                config.database = database.to_owned();
            }
        }

        Ok(config)
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
    /// Maximum number of contract addresses bundled into a single eth_getLogs request.
    pub max_batch_addresses: usize,
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

        let clickhouse = ClickHouseConfig::from_environment()?;

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
            max_batch_addresses: read_positive_usize_env(
                "EVENTLAKE_COLLECTOR_MAX_BATCH_ADDRESSES",
                "50",
            )?,
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
            secure: false,
        };

        assert_eq!(configuration.url(), "http://clickhouse:8123");
    }

    #[test]
    fn clickhouse_url_targets_the_https_endpoint_when_secure() {
        let configuration = ClickHouseConfig {
            host: "clickhouse.cloud".to_owned(),
            port: 8443,
            user: "default".to_owned(),
            password: "secret".to_owned(),
            database: "eventlake".to_owned(),
            enabled: true,
            secure: true,
        };

        assert_eq!(configuration.url(), "https://clickhouse.cloud:8443");
    }

    #[test]
    fn clickhouse_config_from_url_standard() {
        let config = ClickHouseConfig::from_url(
            "http://eventlake:mypassword@clickhouse.local:8123/eventlake",
        )
        .expect("valid standard URL parses");

        assert_eq!(config.host, "clickhouse.local");
        assert_eq!(config.port, 8123);
        assert_eq!(config.user, "eventlake");
        assert_eq!(config.password, "mypassword");
        assert_eq!(config.database, "eventlake");
        assert!(!config.secure);
        assert!(config.enabled);
        assert_eq!(config.url(), "http://clickhouse.local:8123");
    }

    #[test]
    fn clickhouse_config_from_url_clickhouse_scheme() {
        let config = ClickHouseConfig::from_url("clickhouse://admin:secret123@10.0.0.5/analytics")
            .expect("clickhouse:// scheme parses");

        assert_eq!(config.host, "10.0.0.5");
        assert_eq!(config.port, 8123);
        assert_eq!(config.user, "admin");
        assert_eq!(config.password, "secret123");
        assert_eq!(config.database, "analytics");
        assert!(!config.secure);
        assert!(config.enabled);
        assert_eq!(config.url(), "http://10.0.0.5:8123");
    }

    #[test]
    fn clickhouse_config_from_url_https_and_clickhouses() {
        let config = ClickHouseConfig::from_url("https://user:pass@ch.mycorp.internal:8443/lake")
            .expect("https URL parses");
        assert!(config.secure);
        assert_eq!(config.port, 8443);
        assert_eq!(config.url(), "https://ch.mycorp.internal:8443");

        let config2 = ClickHouseConfig::from_url("clickhouses://user:pass@ch.mycorp.internal/lake")
            .expect("clickhouses scheme parses");
        assert!(config2.secure);
        assert_eq!(config2.port, 8443);
        assert_eq!(config2.url(), "https://ch.mycorp.internal:8443");
    }

    #[test]
    fn clickhouse_config_from_url_percent_encoded_credentials() {
        let config =
            ClickHouseConfig::from_url("http://user%40corp:p%40ss%25word@localhost:8123/my%2Ddb")
                .expect("percent-encoded credentials parse correctly");

        assert_eq!(config.user, "user@corp");
        assert_eq!(config.password, "p@ss%word");
        assert_eq!(config.database, "my-db");
    }

    #[test]
    fn clickhouse_config_from_url_minimal() {
        let config =
            ClickHouseConfig::from_url("http://localhost:8123").expect("minimal url parses");

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8123);
        assert_eq!(config.user, "default");
        assert_eq!(config.password, "");
        assert_eq!(config.database, "default");
        assert!(!config.secure);
    }

    #[test]
    fn clickhouse_config_from_url_invalid_cases() {
        assert!(ClickHouseConfig::from_url("").is_err());
        assert!(ClickHouseConfig::from_url("   ").is_err());
        assert!(ClickHouseConfig::from_url("ftp://localhost:8123/db").is_err());
        assert!(ClickHouseConfig::from_url("not a url").is_err());
    }
}
