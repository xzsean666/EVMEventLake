use std::{sync::Arc, time::Duration};

use reqwest::Client;
use sqlx::PgPool;

use crate::configuration::ApplicationConfiguration;

#[derive(Clone)]
pub struct ApplicationState {
    pub configuration: Arc<ApplicationConfiguration>,
    pub pool: PgPool,
    pub http_client: Client,
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
        }
    }
}
