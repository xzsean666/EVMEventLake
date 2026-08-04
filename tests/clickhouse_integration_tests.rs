#![cfg(feature = "clickhouse")]

use std::{env, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Utc;
use eventlake::{
    api,
    app::application_state::ApplicationState,
    clickhouse::{self, IndexedEvent},
    configuration::{
        ApplicationConfiguration, AuthConfiguration, BackgroundConfiguration, ClickHouseConfig,
        DatabaseConfiguration, HttpConfiguration, TelemetryConfiguration,
    },
    indexing::{self, DecodedFieldValue},
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

const CONTRACT_ADDRESS: &str = "0x2222222222222222222222222222222222222222";
const FROM_ADDRESS: &str = "0x1111111111111111111111111111111111111111";
const TOPIC0: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

#[tokio::test]
async fn mirrors_events_routes_search_and_hides_tombstones() -> anyhow::Result<()> {
    if env::var("EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION")
        .ok()
        .as_deref()
        != Some("true")
    {
        eprintln!("skipping ClickHouse integration: set EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true");
        return Ok(());
    }

    let clickhouse_configuration = ClickHouseConfig {
        host: read_env("EVENTLAKE_CLICKHOUSE_HOST", "127.0.0.1"),
        port: read_env("EVENTLAKE_CLICKHOUSE_PORT", "8123").parse()?,
        user: read_env("EVENTLAKE_CLICKHOUSE_USER", "eventlake"),
        password: read_env("EVENTLAKE_CLICKHOUSE_PASSWORD", "eventlake"),
        database: read_env("EVENTLAKE_CLICKHOUSE_DB", "eventlake"),
        enabled: true,
    };
    let client = clickhouse::connect(&clickhouse_configuration)
        .await?
        .expect("enabled configuration returns a client");

    let event_id = Uuid::new_v4();
    let raw_log_id = Uuid::new_v4();
    let event_name = format!("ClickHouseIntegration{event_id}");
    let event = IndexedEvent {
        id: event_id,
        raw_log_id,
        subscription_id: Some(Uuid::new_v4()),
        chain_id: 31_337,
        block_number: 123_456,
        block_hash: "0xabc".to_owned(),
        transaction_hash: format!("0x{event_id:x}"),
        log_index: 7,
        contract_address: CONTRACT_ADDRESS.to_owned(),
        event_name: event_name.clone(),
        topic0: TOPIC0.to_owned(),
        abi_id: Some(Uuid::new_v4()),
        indexed_fields: json!({ "from": FROM_ADDRESS }),
        non_indexed_fields: json!({ "value": "1234" }),
        fields: vec![
            DecodedFieldValue {
                field_name: "from".to_owned(),
                field_type: "address".to_owned(),
                normalized_value: FROM_ADDRESS.to_owned(),
                json_value: json!(FROM_ADDRESS),
            },
            DecodedFieldValue {
                field_name: "value".to_owned(),
                field_type: "uint256".to_owned(),
                normalized_value: "1234".to_owned(),
                json_value: json!("1234"),
            },
        ],
        is_removed: false,
        decoded_at: Utc::now(),
    };
    indexing::mirror_decoded_event(&client, event.clone()).await?;

    let state = ApplicationState::new(test_configuration(clickhouse_configuration), lazy_pool()?)
        .with_clickhouse(client.clone());
    let router = api::routes::build_router(state);

    let response = search(
        &router,
        json!({
            "filters": [
                { "field": "event_name", "operator": "eq", "value": event_name },
                { "field": "address", "operator": "eq", "value": FROM_ADDRESS },
                { "field": "field.value", "operator": "eq", "value": "1234" }
            ]
        }),
    )
    .await?;
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(response.1["data"][0]["id"], json!(event_id));

    let mut removed_event = event;
    removed_event.is_removed = true;
    clickhouse::write_indexed_event(&client, removed_event).await?;

    let response = search(
        &router,
        json!({
            "filters": [
                { "field": "event_name", "operator": "eq", "value": event_name }
            ]
        }),
    )
    .await?;
    assert_eq!(response.0, StatusCode::OK);
    assert!(
        response.1["data"]
            .as_array()
            .expect("data is an array")
            .is_empty()
    );

    Ok(())
}

fn lazy_pool() -> anyhow::Result<sqlx::PgPool> {
    Ok(PgPoolOptions::new().connect_lazy("postgres://eventlake:eventlake@localhost/eventlake")?)
}

fn test_configuration(clickhouse: ClickHouseConfig) -> ApplicationConfiguration {
    ApplicationConfiguration {
        http: HttpConfiguration {
            host: "127.0.0.1".parse().expect("test host parses"),
            port: 0,
            cors_allowed_origins: Vec::new(),
        },
        database: DatabaseConfiguration {
            database_url: "postgres://eventlake:eventlake@localhost/eventlake".to_owned(),
            max_connections: 1,
        },
        clickhouse,
        auth: AuthConfiguration {
            jwt_secret: "test-secret".to_owned(),
            require_authentication: false,
        },
        background: BackgroundConfiguration {
            workers_enabled: false,
            worker_tick: Duration::from_secs(1),
            decode_batch_size: 1,
            partition_tick: Duration::from_secs(1),
        },
        telemetry: TelemetryConfiguration {
            log_level: "warn".to_owned(),
            json_logs: false,
        },
    }
}

async fn search(router: &axum::Router, body: Value) -> anyhow::Result<(StatusCode, Value)> {
    let request = Request::builder()
        .method("POST")
        .uri("/api/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

fn read_env(name: &str, default_value: &str) -> String {
    env::var(name).unwrap_or_else(|_| default_value.to_owned())
}
