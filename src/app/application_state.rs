use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use reqwest::Client;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{abi_registry::CachedAbi, configuration::ApplicationConfiguration};

/// Parsed ABIs keyed by their immutable `abi_id`. Decoding happens once per log, so
/// caching the parsed ABI avoids re-reading and re-parsing the JSON on every event.
pub type AbiCache = Arc<RwLock<HashMap<Uuid, Arc<CachedAbi>>>>;

#[derive(Clone)]
pub struct ApplicationState {
    pub configuration: Arc<ApplicationConfiguration>,
    pub pool: PgPool,
    pub http_client: Client,
    pub abi_cache: AbiCache,
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
            abi_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
