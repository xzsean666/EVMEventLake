#![cfg(feature = "clickhouse")]

use std::{env, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use eventlake::{
    api,
    app::application_state::ApplicationState,
    clickhouse::{self, RawLog},
    configuration::{
        ApplicationConfiguration, AuthConfiguration, BackgroundConfiguration,
        BlockTransactionConfiguration, ClickHouseConfig, DatabaseConfiguration, HttpConfiguration,
        TelemetryConfiguration,
    },
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

    let mut clickhouse_configuration = if let Ok(url) = env::var("EVENTLAKE_CLICKHOUSE_URL") {
        ClickHouseConfig::from_url(&url)?
    } else {
        ClickHouseConfig {
            host: read_env("EVENTLAKE_CLICKHOUSE_HOST", "127.0.0.1"),
            port: read_env("EVENTLAKE_CLICKHOUSE_PORT", "8123").parse()?,
            user: read_env("EVENTLAKE_CLICKHOUSE_USER", "eventlake"),
            password: read_env("EVENTLAKE_CLICKHOUSE_PASSWORD", "eventlake"),
            database: read_env("EVENTLAKE_CLICKHOUSE_DB", "eventlake"),
            enabled: true,
            secure: false,
        }
    };
    clickhouse_configuration.enabled = true;
    let client = clickhouse::connect(&clickhouse_configuration)
        .await?
        .expect("enabled configuration returns a client");

    let raw_log_id = Uuid::new_v4();
    let sub_id = Uuid::new_v4();
    let raw_log = RawLog {
        id: raw_log_id,
        subscription_id: Some(sub_id),
        chain_id: 31_337,
        block_number: 123_456,
        block_hash: "0xabc".to_owned(),
        transaction_hash: format!("0x{:x}", raw_log_id.as_u128()),
        transaction_index: 3,
        log_index: 7,
        contract_address: CONTRACT_ADDRESS.to_owned(),
        topics: vec![TOPIC0.to_owned(), format!("0x{:064x}", 1)],
        data: "0x1234".to_owned(),
        is_removed: false,
    };
    clickhouse::write_raw_logs(&client, &[raw_log]).await?;

    let state = ApplicationState::new(test_configuration(clickhouse_configuration), lazy_pool()?)
        .with_clickhouse(client.clone());
    let router = api::routes::build_router(state);

    let raw_response = search_raw_logs(
        &router,
        json!({
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "block_number", "operator": "eq", "value": 123456 },
                { "field": "topic0", "operator": "eq", "value": TOPIC0 }
            ]
        }),
    )
    .await?;
    assert_eq!(raw_response.0, StatusCode::OK);
    assert_eq!(raw_response.1["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(raw_response.1["data"][0]["data"], json!("0x1234"));

    clickhouse::invalidate_from_block(&client, 31_337, 123_456).await?;
    let raw_response = search_raw_logs(
        &router,
        json!({
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "block_number", "operator": "eq", "value": 123456 },
                { "field": "topic0", "operator": "eq", "value": TOPIC0 }
            ]
        }),
    )
    .await?;
    assert_eq!(raw_response.0, StatusCode::OK);
    assert!(
        raw_response.1["data"]
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
            max_batch_addresses: 50,
            collector_concurrency: 4,
        },
        block_transaction: BlockTransactionConfiguration {
            enabled: false,
            batch_size: 10,
            max_concurrency: 2,
            reorg_window: 32,
            max_response_bytes: 67108864,
        },
        rpc_pool: Default::default(),
        telemetry: TelemetryConfiguration {
            log_level: "warn".to_owned(),
            json_logs: false,
        },
    }
}



async fn search_raw_logs(
    router: &axum::Router,
    body: Value,
) -> anyhow::Result<(StatusCode, Value)> {
    let request = Request::builder()
        .method("POST")
        .uri("/api/raw-logs/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[tokio::test]
async fn blocks_and_transactions_write_and_query_and_reorg() -> anyhow::Result<()> {
    if env::var("EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION")
        .ok()
        .as_deref()
        != Some("true")
    {
        eprintln!("skipping ClickHouse integration: set EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true");
        return Ok(());
    }

    let mut clickhouse_configuration = if let Ok(url) = env::var("EVENTLAKE_CLICKHOUSE_URL") {
        ClickHouseConfig::from_url(&url)?
    } else {
        ClickHouseConfig {
            host: read_env("EVENTLAKE_CLICKHOUSE_HOST", "127.0.0.1"),
            port: read_env("EVENTLAKE_CLICKHOUSE_PORT", "8123").parse()?,
            user: read_env("EVENTLAKE_CLICKHOUSE_USER", "eventlake"),
            password: read_env("EVENTLAKE_CLICKHOUSE_PASSWORD", "eventlake"),
            database: read_env("EVENTLAKE_CLICKHOUSE_DB", "eventlake"),
            enabled: true,
            secure: false,
        }
    };
    clickhouse_configuration.enabled = true;
    let client = clickhouse::connect(&clickhouse_configuration)
        .await?
        .expect("enabled configuration returns a client");

    let chain_id = 99999;
    let block1 = eventlake::rpc_pool::evm_rpc_client::DecodedBlock {
        chain_id,
        block_number: 100,
        block_hash: "0x0000000000000000000000000000000000000000000000000000000000000100".to_owned(),
        parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000099"
            .to_owned(),
        timestamp: 1700000000,
        gas_limit: "30000000".to_owned(),
        gas_used: "21000".to_owned(),
        base_fee_per_gas: Some("1000000000".to_owned()),
        beneficiary: Some(FROM_ADDRESS.to_owned()),
        transactions_root: None,
        receipts_root: None,
        state_root: None,
        size: Some("500".to_owned()),
        withdrawals_root: None,
        blob_gas_used: None,
        excess_blob_gas: None,
        parent_beacon_block_root: None,
        transaction_count: 1,
        transactions: vec![eventlake::rpc_pool::evm_rpc_client::DecodedTransaction {
            chain_id,
            tx_hash: "0x00000000000000000000000000000000000000000000000000000000000000a1"
                .to_owned(),
            block_number: 100,
            transaction_index: 0,
            from_address: FROM_ADDRESS.to_owned(),
            to_address: Some(CONTRACT_ADDRESS.to_owned()),
            value: "1000000000000000000".to_owned(),
            nonce: "1".to_owned(),
            gas: "21000".to_owned(),
            gas_price: Some("20000000000".to_owned()),
            max_fee_per_gas: Some("30000000000".to_owned()),
            max_priority_fee_per_gas: Some("1000000000".to_owned()),
            tx_type: Some(2),
            method_id: Some("0xa9059cbb".to_owned()),
            status: None,
            gas_used: None,
            effective_gas_price: None,
            l1_fee: None,
        }],
    };

    clickhouse::write_blocks_and_transactions(&client, &[block1]).await?;

    let found_block = clickhouse::get_block_by_number(&client, chain_id, 100).await?;
    assert!(found_block.is_some());
    let block_row = found_block.unwrap();
    assert_eq!(block_row.block_number, 100);
    assert_eq!(block_row.gas_limit, "30000000");

    let found_by_hash = clickhouse::get_block_by_hash(
        &client,
        chain_id,
        "0x0000000000000000000000000000000000000000000000000000000000000100",
    )
    .await?;
    assert!(found_by_hash.is_some());

    let txs = clickhouse::get_block_transactions(&client, chain_id, 100, 10, None).await?;
    assert_eq!(txs.len(), 1);
    assert_eq!(
        txs[0].tx_hash,
        "0x00000000000000000000000000000000000000000000000000000000000000a1"
    );

    let tx_by_hash = clickhouse::get_transaction_by_hash(
        &client,
        chain_id,
        "0x00000000000000000000000000000000000000000000000000000000000000a1",
    )
    .await?;
    assert!(tx_by_hash.is_some());

    let state = ApplicationState::new(test_configuration(clickhouse_configuration), lazy_pool()?)
        .with_clickhouse(client.clone());
    let router = api::routes::build_router(state);

    // Query block detail API by height
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/chains/{chain_id}/blocks/100"))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"]["block_number"], 100);
    assert_eq!(json["data"]["gas_limit"], "30000000");

    // Query block detail API by hash
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/chains/{chain_id}/blocks/0x0000000000000000000000000000000000000000000000000000000000000100"))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);

    // Query block transactions API
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/{chain_id}/blocks/100/transactions?limit=10"
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        json["data"][0]["tx_hash"],
        "0x00000000000000000000000000000000000000000000000000000000000000a1"
    );

    // Query transaction detail API
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/chains/{chain_id}/transactions/0x00000000000000000000000000000000000000000000000000000000000000a1"))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"]["value"], "1000000000000000000");

    // Query address transactions API
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/{chain_id}/addresses/{FROM_ADDRESS}/transactions?direction=from"
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"].as_array().map(Vec::len), Some(1));

    let addr_txs = clickhouse::get_address_transactions(
        &client,
        chain_id,
        FROM_ADDRESS,
        "from",
        None,
        None,
        10,
        None,
    )
    .await?;
    assert_eq!(addr_txs.len(), 1);

    // Reorg tombstone test
    clickhouse::invalidate_blocks_and_transactions_from_block(&client, chain_id, 100).await?;

    let after_reorg_block = clickhouse::get_block_by_number(&client, chain_id, 100).await?;
    assert!(after_reorg_block.is_none());

    let after_reorg_tx = clickhouse::get_transaction_by_hash(
        &client,
        chain_id,
        "0x00000000000000000000000000000000000000000000000000000000000000a1",
    )
    .await?;
    assert!(after_reorg_tx.is_none());

    // Block query after reorg returns 404
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/chains/{chain_id}/blocks/100"))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    Ok(())
}

fn read_env(name: &str, default_value: &str) -> String {
    env::var(name).unwrap_or_else(|_| default_value.to_owned())
}
