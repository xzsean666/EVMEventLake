use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use reqwest::Client;
use sqlx::PgPool;
use uuid::Uuid;

use crate::configuration::ApplicationConfiguration;

#[derive(Clone)]
pub struct ApplicationState {
    pub configuration: Arc<ApplicationConfiguration>,
    pub pool: PgPool,
    pub http_client: Client,
    pub api_key_last_used: Arc<RwLock<HashMap<Uuid, std::time::Instant>>>,
    #[cfg(feature = "clickhouse")]
    clickhouse: Arc<RwLock<Option<clickhouse::Client>>>,
}

impl ApplicationState {
    pub fn new(configuration: ApplicationConfiguration, pool: PgPool) -> Self {
        Self {
            configuration: Arc::new(configuration),
            pool,
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client builds"),
            api_key_last_used: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "clickhouse")]
            clickhouse: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(feature = "clickhouse")]
    pub fn with_clickhouse(self, clickhouse: clickhouse::Client) -> Self {
        self.set_clickhouse_client(clickhouse);
        self
    }

    #[cfg(feature = "clickhouse")]
    pub fn clickhouse_client(&self) -> Option<clickhouse::Client> {
        self.clickhouse
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(feature = "clickhouse")]
    pub fn set_clickhouse_client(&self, clickhouse: clickhouse::Client) {
        *self
            .clickhouse
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(clickhouse);
    }

    #[cfg(feature = "clickhouse")]
    pub fn clear_clickhouse_client(&self) {
        *self
            .clickhouse
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}
