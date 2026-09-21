use std::{env, net::SocketAddr, time::Duration};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
    routing::post,
};
use chrono::Utc;
use eventlake::{
    api, app::application_state::ApplicationState, auth, block_transaction, clickhouse, collector,
    configuration, database, reorg, rpc_pool, shared::hex::parse_hex_u64,
};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

const CONTRACT_ADDRESS: &str = "0x2222222222222222222222222222222222222222";
const BATCH_CONTRACT_A: &str = "0x4444444444444444444444444444444444444444";
const BATCH_CONTRACT_B: &str = "0x5555555555555555555555555555555555555555";
const FROM_ADDRESS: &str = "0x1111111111111111111111111111111111111111";
const TO_ADDRESS: &str = "0x3333333333333333333333333333333333333333";
const TRANSFER_TOPIC0: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const BASE_CHAIN_ID: i64 = 8453;
const BASE_USDC_ADDRESS: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
const DEFAULT_LIVE_BASE_RPC_URL: &str = "https://mainnet.base.org";
const LIVE_SAFE_CONFIRMATION_DEPTH: i64 = 20;
const LIVE_DISCOVERY_WINDOW: i64 = 10;
const LIVE_COLLECTION_WINDOW: i64 = 1;
const LIVE_DISCOVERY_CHUNKS: i64 = 30;

#[derive(Debug)]
struct LiveChainSample {
    from_block: i64,
    to_block: i64,
    log_count: usize,
    transfer_count: usize,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_eventlake_workflow_on_real_sqlite() -> anyhow::Result<()> {
    unsafe { std::env::set_var("EVENTLAKE_ALLOW_PRIVATE_RPC", "true"); }
    let Some(database_url) = test_database_url() else {
        eprintln!("skipping real database e2e: database url not available");
        return Ok(());
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    reset_eventlake_namespace_before_migration(&pool).await?;
    database::migrate(&pool).await?;
    reset_eventlake_tables(&pool).await?;

    let rpc_url = spawn_json_rpc_fixture().await?;
    let state = build_test_state(database_url.clone(), pool.clone(), false);
    let router = api::routes::build_router(state.clone());

    assert_ok(get(&router, "/health/live").await?, StatusCode::OK);
    assert_ok(get(&router, "/health/ready").await?, StatusCode::OK);
    let openapi_response = get(&router, "/api/openapi.json").await?;
    assert_eq!(openapi_response.0, StatusCode::OK);
    assert_eq!(openapi_response.1["openapi"], "3.1.0");
    assert!(
        openapi_response.1["paths"]
            .get("/api/raw-logs/search")
            .is_some()
    );
    assert!(
        openapi_response.1["paths"]
            .get("/api/subscriptions")
            .is_some()
    );
    assert!(
        openapi_response.1["paths"]
            .get("/api/subscriptions/batch")
            .is_some()
    );

    let api_key_response = post_json(
        &router,
        "/api/auth/api-keys",
        json!({ "name": "e2e-admin", "role": "admin" }),
    )
    .await?;
    assert_ok(api_key_response.clone(), StatusCode::OK);
    assert!(
        response_data(&api_key_response.1)
            .get("api_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .starts_with("evl_")
    );

    let chain_response = post_json(
        &router,
        "/api/chains",
        json!({
            "chain_id": 31337,
            "name": "Local E2E",
            "native_token_symbol": "ETH",
            "safe_confirmation_depth": 0,
            "default_max_block_window": 50,
            "rpc_notes": "deterministic e2e fixture"
        }),
    )
    .await?;
    assert_ok(chain_response.clone(), StatusCode::OK);
    assert_eq!(response_data(&chain_response.1)["chain_id"], 31337);
    assert_ok(get(&router, "/api/chains/31337").await?, StatusCode::OK);
    assert_ok(get(&router, "/api/chains").await?, StatusCode::OK);

    let batch_response = post_json(
        &router,
        "/api/subscriptions/batch",
        json!({
            "chain_id": 31337,
            "contract_addresses": [BATCH_CONTRACT_A, BATCH_CONTRACT_A, BATCH_CONTRACT_B],
            "start_block": 100
        }),
    )
    .await?;
    assert_ok(batch_response.clone(), StatusCode::OK);
    let batch_records = response_data(&batch_response.1)
        .as_array()
        .expect("batch response data is an array");
    assert_eq!(batch_records.len(), 2);
    assert!(
        batch_records
            .iter()
            .all(|record| record["abi_id"].is_null())
    );
    assert!(
        batch_records
            .iter()
            .all(|record| record["start_block"] == 100 && record["current_block"] == 100)
    );

    let batch_retry = post_json(
        &router,
        "/api/subscriptions/batch",
        json!({
            "chain_id": 31337,
            "contract_addresses": [BATCH_CONTRACT_B, BATCH_CONTRACT_A],
            "start_block": 1
        }),
    )
    .await?;
    assert_ok(batch_retry.clone(), StatusCode::OK);
    let retry_records = response_data(&batch_retry.1)
        .as_array()
        .expect("batch retry data is an array");
    assert_eq!(retry_records.len(), 2);
    assert_eq!(
        batch_records
            .iter()
            .map(|record| record["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>(),
        retry_records
            .iter()
            .map(|record| record["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>()
    );

    assert_error(
        post_json(
            &router,
            "/api/chains",
            json!({
                "chain_id": -1,
                "name": "Invalid",
                "native_token_symbol": "ETH",
                "safe_confirmation_depth": -1
            }),
        )
        .await?,
        StatusCode::BAD_REQUEST,
    );

    assert_error(
        post_json(
            &router,
            "/api/subscriptions",
            json!({
                "chain_id": 31337,
                "collection_scope": "all_events",
                "start_block": 100
            }),
        )
        .await?,
        StatusCode::BAD_REQUEST,
    );

    assert_error(
        post_json(
            &router,
            "/api/rpc-endpoints",
            json!({ "chain_id": 31337, "url": "ftp://example.invalid", "weight": 0 }),
        )
        .await?,
        StatusCode::BAD_REQUEST,
    );

    let rpc_response = post_json(
        &router,
        "/api/rpc-endpoints",
        json!({ "chain_id": 31337, "url": rpc_url, "weight": 100 }),
    )
    .await?;
    assert_ok(rpc_response.clone(), StatusCode::OK);
    let rpc_id = uuid_from_response(&rpc_response.1, "id");
    assert_ok(
        post_json(
            &router,
            &format!("/api/rpc-endpoints/{rpc_id}/check"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        post_json(
            &router,
            &format!("/api/rpc-endpoints/{rpc_id}/disable"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        post_json(
            &router,
            &format!("/api/rpc-endpoints/{rpc_id}/enable"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        get(&router, &format!("/api/rpc-endpoints/{rpc_id}")).await?,
        StatusCode::OK,
    );
    assert_ok(get(&router, "/api/rpc-endpoints").await?, StatusCode::OK);

    // Test deleting RPC endpoint
    let rpc_to_delete = post_json(
        &router,
        "/api/rpc-endpoints",
        json!({ "chain_id": 31337, "url": "https://temp-rpc.example.com", "weight": 50 }),
    )
    .await?;
    assert_ok(rpc_to_delete.clone(), StatusCode::OK);
    let temp_rpc_id = uuid_from_response(&rpc_to_delete.1, "id");
    assert_ok(
        delete(&router, &format!("/api/rpc-endpoints/{temp_rpc_id}")).await?,
        StatusCode::OK,
    );
    assert_error(
        get(&router, &format!("/api/rpc-endpoints/{temp_rpc_id}")).await?,
        StatusCode::NOT_FOUND,
    );

    // Test seeding RPC endpoints from JSON
    let seed_json = r#"[
        {"chain_id": 31337, "url": "https://seeded-1.example.com", "weight": 120},
        {"chain_id": 99999, "url": "https://custom-chain-rpc.example.com", "weight": 80, "chain_name": "Custom Testnet", "native_token_symbol": "TEST"}
    ]"#;
    let seeded = rpc_pool::seed_rpc_endpoints_from_json(&pool, seed_json).await?;
    assert_eq!(seeded, 2);

    // Idempotent seeding (no duplicate insertions)
    let reseeded = rpc_pool::seed_rpc_endpoints_from_json(&pool, seed_json).await?;
    assert_eq!(reseeded, 0);

    // Verify custom chain was created
    assert_ok(get(&router, "/api/chains/99999").await?, StatusCode::OK);

    let abi_id: Option<Uuid> = None;

    assert_error(
        post_json(
            &router,
            "/api/subscriptions",
            json!({
                "chain_id": 31337,
                "contract_address": CONTRACT_ADDRESS,
                "abi_id": abi_id,
                "start_block": -1
            }),
        )
        .await?,
        StatusCode::BAD_REQUEST,
    );

    let subscription_response = post_json(
        &router,
        "/api/subscriptions",
        json!({
            "chain_id": 31337,
            "contract_address": CONTRACT_ADDRESS,
            "abi_id": abi_id,
            "start_block": 100,
            "realtime_enabled": true
        }),
    )
    .await?;
    assert_ok(subscription_response.clone(), StatusCode::OK);
    let subscription_id = uuid_from_response(&subscription_response.1, "id");
    assert_eq!(response_data(&subscription_response.1)["start_block"], 100);
    assert_eq!(
        response_data(&subscription_response.1)["current_block"],
        100
    );

    let duplicate_subscription_response = post_json(
        &router,
        "/api/subscriptions",
        json!({
            "chain_id": 31337,
            "contract_address": CONTRACT_ADDRESS,
            "abi_id": abi_id,
            "start_block": 1,
            "realtime_enabled": true
        }),
    )
    .await?;
    assert_ok(duplicate_subscription_response.clone(), StatusCode::OK);
    assert_eq!(
        uuid_from_response(&duplicate_subscription_response.1, "id"),
        subscription_id
    );

    assert_ok(
        post_json(
            &router,
            &format!("/api/subscriptions/{subscription_id}/pause"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        post_json(
            &router,
            &format!("/api/subscriptions/{subscription_id}/resume"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        get(&router, &format!("/api/subscriptions/{subscription_id}")).await?,
        StatusCode::OK,
    );
    assert_ok(get(&router, "/api/subscriptions").await?, StatusCode::OK);

    collector::worker::collect_once(&state).await?;
    assert_eq!(count_rows(&pool, "eventlake_subscriptions").await?, 3);

    let dashboard = get(&router, "/api/dashboard").await?;
    assert_ok(dashboard.clone(), StatusCode::OK);
    assert_eq!(response_data(&dashboard.1)["active_jobs"], 3);
    assert_eq!(response_data(&dashboard.1)["total_decoded_events"], 0);

    assert_authentication_modes(&database_url, pool.clone()).await?;

    let reorg_result = reorg::observe_block(
        &pool,
        31337,
        100,
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0",
    )
    .await?;
    assert!(matches!(
        reorg_result,
        reorg::BlockCheckpointResult::ReorgDetected { .. }
    ));

    assert_ok(
        post_json(
            &router,
            &format!("/api/subscriptions/{subscription_id}/pause"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    assert_ok(
        delete(&router, &format!("/api/subscriptions/{subscription_id}")).await?,
        StatusCode::OK,
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_chain_collects_and_searches_raw_base_usdc_logs() -> anyhow::Result<()> {
    if env::var("EVENTLAKE_RUN_LIVE_CHAIN_E2E").ok().as_deref() != Some("true") {
        eprintln!(
            "skipping live chain e2e: set EVENTLAKE_RUN_LIVE_CHAIN_E2E=true to run against Base"
        );
        return Ok(());
    }

    let Some(database_url) = test_database_url() else {
        eprintln!("skipping live chain e2e: .env.test DATABASE_URL is not configured");
        return Ok(());
    };

    let live_rpc_url =
        env::var("EVENTLAKE_LIVE_RPC_URL").unwrap_or_else(|_| DEFAULT_LIVE_BASE_RPC_URL.to_owned());
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let sample = discover_live_base_usdc_sample(&http_client, &live_rpc_url).await?;
    eprintln!(
        "live e2e sample: Base USDC blocks {}..={}, {} logs, {} transfers",
        sample.from_block, sample.to_block, sample.log_count, sample.transfer_count
    );

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    reset_eventlake_namespace_before_migration(&pool).await?;
    database::migrate(&pool).await?;
    reset_eventlake_tables(&pool).await?;

    let mut clickhouse_config = if let Ok(url) = env::var("EVENTLAKE_CLICKHOUSE_URL") {
        configuration::ClickHouseConfig::from_url(&url)?
    } else {
        configuration::ClickHouseConfig {
            host: "127.0.0.1".to_owned(),
            port: 8123,
            user: "eventlake".to_owned(),
            password: "eventlake".to_owned(),
            database: "eventlake".to_owned(),
            enabled: true,
            secure: false,
            ..Default::default()
        }
    };
    clickhouse_config.enabled = true;
    let ch_client = eventlake::clickhouse::connect(&clickhouse_config)
        .await?
        .expect("enabled configuration returns a client");

    let state = build_test_state_with_clickhouse(
        database_url.clone(),
        pool.clone(),
        false,
        clickhouse_config,
        false,
    )
    .with_clickhouse(ch_client.clone());
    let router = api::routes::build_router(state.clone());

    let chain_response = post_json(
        &router,
        "/api/chains",
        json!({
            "chain_id": BASE_CHAIN_ID,
            "name": "Base",
            "native_token_symbol": "ETH",
            "safe_confirmation_depth": LIVE_SAFE_CONFIRMATION_DEPTH,
            "default_min_block_window": 1,
            "default_max_block_window": LIVE_COLLECTION_WINDOW,
            "rpc_notes": "live e2e sample"
        }),
    )
    .await?;
    assert_ok(chain_response, StatusCode::OK);

    let rpc_response = post_json(
        &router,
        "/api/rpc-endpoints",
        json!({ "chain_id": BASE_CHAIN_ID, "url": live_rpc_url, "weight": 100 }),
    )
    .await?;
    assert_ok(rpc_response.clone(), StatusCode::OK);
    let rpc_id = uuid_from_response(&rpc_response.1, "id");
    assert_ok(
        post_json(
            &router,
            &format!("/api/rpc-endpoints/{rpc_id}/check"),
            json!({}),
        )
        .await?,
        StatusCode::OK,
    );
    eprintln!("live e2e rpc health check passed");

    let abi_id: Option<Uuid> = None;

    let subscription_response = post_json(
        &router,
        "/api/subscriptions",
        json!({
            "chain_id": BASE_CHAIN_ID,
            "contract_address": BASE_USDC_ADDRESS,
            "abi_id": abi_id,
            "start_block": sample.from_block,
            "realtime_enabled": false,
            "min_block_window": 1,
            "max_block_window": LIVE_COLLECTION_WINDOW
        }),
    )
    .await?;
    assert_ok(subscription_response, StatusCode::OK);

    collector::worker::collect_once(&state).await?;

    let raw_count = count_raw_logs_in_clickhouse(&ch_client, BASE_CHAIN_ID, BASE_USDC_ADDRESS).await?;
    eprintln!("live e2e collected {raw_count} raw logs");
    assert!(
        raw_count > 0,
        "expected live Base USDC logs in {}..={}, discovery saw {} logs and {} transfers",
        sample.from_block,
        sample.to_block,
        sample.log_count,
        sample.transfer_count
    );

    let search_response = post_json(
        &router,
        "/api/raw-logs/search",
        json!({
            "page": 1,
            "limit": 10,
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": BASE_CHAIN_ID },
                { "field": "contract_address", "operator": "eq", "value": BASE_USDC_ADDRESS },
                { "field": "topic0", "operator": "eq", "value": TRANSFER_TOPIC0 }
            ],
            "sort": { "field": "block_number", "direction": "desc" }
        }),
    )
    .await?;
    assert_ok(search_response.clone(), StatusCode::OK);
    assert!(
        !response_data(&search_response.1)
            .as_array()
            .unwrap()
            .is_empty(),
        "expected live raw Transfer logs to be searchable"
    );

    Ok(())
}

async fn assert_authentication_modes(database_url: &str, pool: SqlitePool) -> anyhow::Result<()> {
    let admin_key = "e2e-admin-secret";
    let read_only_key = "e2e-readonly-secret";

    sqlx::query(
        r#"
        INSERT INTO eventlake_api_keys (id, name, key_hash, role)
        VALUES ($1, 'e2e-auth-admin', $2, 'admin'),
               ($3, 'e2e-auth-readonly', $4, 'read_only')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(auth::hash_api_key(admin_key))
    .bind(Uuid::new_v4())
    .bind(auth::hash_api_key(read_only_key))
    .execute(&pool)
    .await?;

    let state = build_test_state(database_url.to_owned(), pool, true);
    let router = api::routes::build_router(state);

    assert_eq!(
        get(&router, "/api/chains").await?.0,
        StatusCode::UNAUTHORIZED
    );

    let read_only_response = request_json_with_headers(
        &router,
        Method::GET,
        "/api/chains",
        None,
        vec![("x-api-key", read_only_key)],
    )
    .await?;
    assert_ok(read_only_response, StatusCode::OK);

    let forbidden_response = request_json_with_headers(
        &router,
        Method::POST,
        "/api/chains",
        Some(json!({
            "chain_id": 31338,
            "name": "Forbidden Chain",
            "native_token_symbol": "ETH"
        })),
        vec![("x-api-key", read_only_key)],
    )
    .await?;
    assert_eq!(forbidden_response.0, StatusCode::FORBIDDEN);

    let admin_response = request_json_with_headers(
        &router,
        Method::POST,
        "/api/chains",
        Some(json!({
            "chain_id": 31338,
            "name": "Admin Chain",
            "native_token_symbol": "ETH"
        })),
        vec![("x-api-key", admin_key)],
    )
    .await?;
    assert_ok(admin_response, StatusCode::OK);

    let jwt = encode(
        &Header::default(),
        &json!({
            "sub": "e2e-jwt-admin",
            "role": "admin",
            "exp": Utc::now().timestamp() + 300
        }),
        &EncodingKey::from_secret(b"test-secret"),
    )?;
    let jwt_response = request_json_with_headers(
        &router,
        Method::GET,
        "/api/chains/31338",
        None,
        vec![("authorization", &format!("Bearer {jwt}"))],
    )
    .await?;
    assert_ok(jwt_response, StatusCode::OK);

    Ok(())
}

async fn discover_live_base_usdc_sample(
    client: &reqwest::Client,
    rpc_url: &str,
) -> anyhow::Result<LiveChainSample> {
    let chain_head = rpc_pool::evm_rpc_client::eth_block_number(client, rpc_url).await?;
    let safe_head = chain_head.saturating_sub(LIVE_SAFE_CONFIRMATION_DEPTH);

    for chunk_index in 0..LIVE_DISCOVERY_CHUNKS {
        let to_block = safe_head.saturating_sub(chunk_index * LIVE_DISCOVERY_WINDOW);
        let from_block = to_block.saturating_sub(LIVE_DISCOVERY_WINDOW - 1).max(1);
        let logs = rpc_pool::evm_rpc_client::eth_get_logs(
            client,
            rpc_url,
            &[BASE_USDC_ADDRESS],
            from_block,
            to_block,
        )
        .await?;

        let mut selected_block = None;
        for log in &logs {
            let has_transfer_topic = log
                .topics
                .first()
                .map(|topic| topic.eq_ignore_ascii_case(TRANSFER_TOPIC0))
                .unwrap_or(false);
            if has_transfer_topic {
                selected_block = Some(parse_hex_u64(&log.block_number)?);
                break;
            }
        }

        if let Some(block_number) = selected_block {
            let mut block_log_count = 0usize;
            let mut block_transfer_count = 0usize;
            for log in &logs {
                if parse_hex_u64(&log.block_number)? == block_number {
                    block_log_count += 1;
                    if log
                        .topics
                        .first()
                        .map(|topic| topic.eq_ignore_ascii_case(TRANSFER_TOPIC0))
                        .unwrap_or(false)
                    {
                        block_transfer_count += 1;
                    }
                }
            }

            return Ok(LiveChainSample {
                from_block: block_number,
                to_block: block_number,
                log_count: block_log_count,
                transfer_count: block_transfer_count,
            });
        }
    }

    anyhow::bail!("no Base USDC Transfer logs found near safe head {safe_head} using {rpc_url}")
}

fn test_database_url() -> Option<String> {
    dotenvy::from_filename(".env.test").ok();
    env::var("DATABASE_URL")
        .or_else(|_| env::var("EVENTLAKE_DATABASE_URL"))
        .ok()
        .or_else(|| {
            let temp_dir = env::temp_dir();
            let db_path = temp_dir.join(format!("eventlake_test_{}.db", Uuid::new_v4()));
            Some(format!("sqlite://{}?mode=rwc", db_path.display()))
        })
}

fn build_test_state(
    database_url: String,
    pool: SqlitePool,
    require_authentication: bool,
) -> ApplicationState {
    build_test_state_with_clickhouse(
        database_url,
        pool,
        require_authentication,
        configuration::ClickHouseConfig::default(),
        false,
    )
}

fn build_test_state_with_clickhouse(
    database_url: String,
    pool: SqlitePool,
    require_authentication: bool,
    clickhouse: configuration::ClickHouseConfig,
    block_transaction_enabled: bool,
) -> ApplicationState {
    ApplicationState::new(
        configuration::ApplicationConfiguration {
            http: configuration::HttpConfiguration {
                host: "127.0.0.1".parse().expect("test host parses"),
                port: 0,
                cors_allowed_origins: Vec::new(),
            },
            database: configuration::DatabaseConfiguration {
                database_url,
                max_connections: 5,
            },
            clickhouse,
            auth: configuration::AuthConfiguration {
                jwt_secret: "test-secret".to_owned(),
                require_authentication,
            },
            background: configuration::BackgroundConfiguration {
                workers_enabled: false,
                worker_tick: Duration::from_millis(50),
                partition_tick: Duration::from_secs(300),
                max_batch_addresses: 50,
                collector_concurrency: 4,
                rpc_healthcheck_enabled: false,
            },
            block_transaction: configuration::BlockTransactionConfiguration {
                enabled: block_transaction_enabled,
                batch_size: 10,
                max_concurrency: 2,
                reorg_window: 32,
                max_response_bytes: 67108864,
            },
            rpc_pool: configuration::RpcPoolConfiguration { seeds_path: None },
            telemetry: configuration::TelemetryConfiguration {
                log_level: "debug".to_owned(),
                json_logs: false,
            },
        },
        pool,
    )
}

async fn reset_eventlake_tables(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM eventlake_api_keys;").execute(pool).await?;
    sqlx::query("DELETE FROM eventlake_block_checkpoints;").execute(pool).await?;
    sqlx::query("DELETE FROM eventlake_subscriptions;").execute(pool).await?;
    sqlx::query("DELETE FROM eventlake_rpc_endpoints;").execute(pool).await?;
    sqlx::query("DELETE FROM eventlake_block_transaction_sync_state;").execute(pool).await?;
    sqlx::query("DELETE FROM eventlake_chains;").execute(pool).await?;
    Ok(())
}

async fn reset_eventlake_namespace_before_migration(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query("DROP TABLE IF EXISTS eventlake_api_keys;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS eventlake_block_checkpoints;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS eventlake_subscriptions;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS eventlake_rpc_endpoints;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS eventlake_chains;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS eventlake_block_transaction_sync_state;").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations;").execute(pool).await?;
    Ok(())
}

async fn spawn_json_rpc_fixture() -> anyhow::Result<String> {
    let router = Router::new().route("/", post(json_rpc_fixture));
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;

    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            eprintln!("json-rpc fixture failed: {error}");
        }
    });

    Ok(format!("http://{address}"))
}

async fn json_rpc_fixture(Json(request): Json<Value>) -> Json<Value> {
    match request {
        Value::Array(items) => {
            let responses: Vec<Value> = items.iter().map(handle_single_rpc_call).collect();
            Json(Value::Array(responses))
        }
        ref single => Json(handle_single_rpc_call(single)),
    }
}

fn handle_single_rpc_call(request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = request.get("id").cloned().unwrap_or_else(|| json!(1));

    match method {
        "eth_blockNumber" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": "0x6f"
        }),
        "eth_getLogs" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": [{
                "address": CONTRACT_ADDRESS,
                "blockHash": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "blockNumber": "0x64",
                "data": uint256_topic_data(1234),
                "logIndex": "0x0",
                "removed": false,
                "topics": [
                    TRANSFER_TOPIC0,
                    address_topic(FROM_ADDRESS),
                    address_topic(TO_ADDRESS)
                ],
                "transactionHash": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "transactionIndex": "0x0"
            }]
        }),
        "eth_getBlockByNumber" => {
            let block_param = request
                .get("params")
                .and_then(|p| p.get(0))
                .and_then(Value::as_str)
                .unwrap_or("0x64");
            let block_num = if block_param.starts_with("0x") {
                u64::from_str_radix(block_param.trim_start_matches("0x"), 16).unwrap_or(100)
            } else {
                100
            };
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "number": format!("0x{:x}", block_num),
                    "hash": format!("0x{:064x}", block_num),
                    "parentHash": format!("0x{:064x}", block_num.saturating_sub(1)),
                    "timestamp": "0x65000000",
                    "gasLimit": "0x1c9c380",
                    "gasUsed": "0x5208",
                    "transactions": [{
                        "hash": format!("0x{:064x}", block_num * 1000 + 1),
                        "blockNumber": format!("0x{:x}", block_num),
                        "transactionIndex": "0x0",
                        "from": FROM_ADDRESS,
                        "to": CONTRACT_ADDRESS,
                        "value": "0xde0b6b3a7640000",
                        "nonce": "0x1",
                        "gas": "0x5208",
                        "gasPrice": "0x4a817c800",
                        "type": "0x2",
                        "input": "0xa9059cbb"
                    }]
                }
            })
        }
        "eth_getBlockReceipts" | "eth_getTransactionReceipt" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": [{
                "transactionHash": "0x00000000000000000000000000000000000000000000000000000000000186a1",
                "transactionIndex": "0x0",
                "blockNumber": "0x64",
                "blockHash": "0x0000000000000000000000000000000000000000000000000000000000000064",
                "from": FROM_ADDRESS,
                "to": CONTRACT_ADDRESS,
                "status": "0x1",
                "gasUsed": "0x5208",
                "effectiveGasPrice": "0x4a817c800"
            }]
        }),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "method not found" }
        }),
    }
}



fn address_topic(address: &str) -> String {
    format!("0x{:0>64}", address.trim_start_matches("0x"))
}

fn uint256_topic_data(value: u64) -> String {
    format!("0x{value:064x}")
}

async fn get(router: &Router, path: &str) -> anyhow::Result<(StatusCode, Value)> {
    request_json(router, Method::GET, path, None).await
}

async fn delete(router: &Router, path: &str) -> anyhow::Result<(StatusCode, Value)> {
    request_json(router, Method::DELETE, path, None).await
}

async fn post_json(
    router: &Router,
    path: &str,
    body: Value,
) -> anyhow::Result<(StatusCode, Value)> {
    request_json(router, Method::POST, path, Some(body)).await
}

async fn request_json(
    router: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<(StatusCode, Value)> {
    request_json_with_headers(router, method, path, body, Vec::new()).await
}

async fn request_json_with_headers(
    router: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    headers: Vec<(&str, &str)>,
) -> anyhow::Result<(StatusCode, Value)> {
    let body = body
        .map(|value| Body::from(value.to_string()))
        .unwrap_or_else(Body::empty);
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");

    for (name, value) in headers {
        builder = builder.header(name, value);
    }

    let response = router.clone().oneshot(builder.body(body)?).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };

    Ok((status, value))
}

fn assert_ok(response: (StatusCode, Value), expected_status: StatusCode) {
    assert_eq!(response.0, expected_status, "response body: {}", response.1);
    assert_eq!(response.1["success"], true, "response body: {}", response.1);
}

fn assert_error(response: (StatusCode, Value), expected_status: StatusCode) {
    assert_eq!(response.0, expected_status, "response body: {}", response.1);
    assert_eq!(
        response.1["success"], false,
        "response body: {}",
        response.1
    );
}

fn response_data(response: &Value) -> &Value {
    response.get("data").expect("response has data")
}

fn uuid_from_response(response: &Value, field: &str) -> Uuid {
    response_data(response)
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("response field is uuid")
}

async fn count_rows(pool: &SqlitePool, table_name: &'static str) -> anyhow::Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table_name}");
    Ok(sqlx::query_as::<_, (i64,)>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await?
        .0)
}

async fn count_raw_logs_in_clickhouse(
    client: &eventlake::clickhouse::Client,
    chain_id: i64,
    contract_address: &str,
) -> anyhow::Result<u64> {
    let sql = format!(
        "SELECT count() FROM raw_logs FINAL WHERE chain_id = {chain_id} AND lower(contract_address) = lower('{contract_address}') AND is_removed = false"
    );
    let count: u64 = client.query(&sql).fetch_one().await?;
    Ok(count)
}

#[tokio::test]
async fn block_transaction_sync_and_storage_guard_workflow() -> anyhow::Result<()> {
    let Some(database_url) = test_database_url() else {
        eprintln!("skipping database E2E: database url not available");
        return Ok(());
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    database::migrate(&pool).await?;
    reset_eventlake_tables(&pool).await?;

    let state = build_test_state(database_url, pool.clone(), false);
    let router = api::routes::build_router(state);

    let chain_id = 8453;
    let create_chain_response = request_json(
        &router,
        Method::POST,
        "/api/chains",
        Some(json!({
            "chain_id": chain_id,
            "name": "Base Mainnet",
            "native_token_symbol": "ETH",
            "safe_confirmation_depth": 32,
            "default_min_block_window": 1,
            "default_max_block_window": 100,
            "rpc_notes": "test chain"
        })),
    )
    .await?;
    assert_ok(create_chain_response, StatusCode::OK);

    // Initial sync status should be 404
    let status_res = request_json(
        &router,
        Method::GET,
        &format!("/api/chains/{chain_id}/sync-status"),
        None,
    )
    .await?;
    assert_error(status_res, StatusCode::NOT_FOUND);

    // Configure sync state
    let sync_config_res = request_json(
        &router,
        Method::PUT,
        &format!("/api/chains/{chain_id}/block-transaction-sync"),
        Some(json!({
            "start_block": 1000,
            "end_block": 2000,
            "batch_size": 20,
            "reorg_window": 16,
            "realtime_enabled": true,
            "status": "pending"
        })),
    )
    .await?;
    assert_ok(sync_config_res, StatusCode::OK);

    // Get sync status
    let status_res = request_json(
        &router,
        Method::GET,
        &format!("/api/chains/{chain_id}/sync-status"),
        None,
    )
    .await?;
    assert_ok(status_res, StatusCode::OK);

    // Pause sync
    let pause_res = request_json(
        &router,
        Method::POST,
        &format!("/api/chains/{chain_id}/block-transaction-sync/pause"),
        None,
    )
    .await?;
    assert_ok(pause_res, StatusCode::OK);

    // Resume sync
    let resume_res = request_json(
        &router,
        Method::POST,
        &format!("/api/chains/{chain_id}/block-transaction-sync/resume"),
        None,
    )
    .await?;
    assert_ok(resume_res, StatusCode::OK);

    // Block query when ClickHouse is disabled returns 503
    let block_res = request_json(
        &router,
        Method::GET,
        &format!("/api/chains/{chain_id}/blocks/1000"),
        None,
    )
    .await?;
    assert_error(block_res, StatusCode::SERVICE_UNAVAILABLE);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_pipeline_mock_rpc_collector_clickhouse_search_and_reorg_e2e() -> anyhow::Result<()> {
    unsafe { std::env::set_var("EVENTLAKE_ALLOW_PRIVATE_RPC", "true"); }
    let Some(database_url) = test_database_url() else {
        eprintln!("skipping full pipeline e2e: database url not available");
        return Ok(());
    };

    let mut clickhouse_config = if let Ok(url) = env::var("EVENTLAKE_CLICKHOUSE_URL") {
        configuration::ClickHouseConfig::from_url(&url)?
    } else {
        configuration::ClickHouseConfig {
            host: "127.0.0.1".to_owned(),
            port: 8123,
            user: "eventlake".to_owned(),
            password: "eventlake".to_owned(),
            database: "eventlake".to_owned(),
            enabled: true,
            secure: false,
            ..Default::default()
        }
    };
    clickhouse_config.enabled = true;
    let ch_client = match clickhouse::connect(&clickhouse_config).await {
        Ok(Some(c)) => c,
        _ => {
            eprintln!("skipping full pipeline e2e: ClickHouse is unreachable on 8123");
            return Ok(());
        }
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    reset_eventlake_namespace_before_migration(&pool).await?;
    database::migrate(&pool).await?;
    reset_eventlake_tables(&pool).await?;

    // Clear ClickHouse test data for chain 31337
    clickhouse::invalidate_from_block(&ch_client, 31337, 0).await?;
    clickhouse::invalidate_blocks_and_transactions_from_block(&ch_client, 31337, 0).await?;

    let rpc_url = spawn_json_rpc_fixture().await?;
    let state = build_test_state_with_clickhouse(
        database_url.clone(),
        pool.clone(),
        false,
        clickhouse_config,
        true,
    )
    .with_clickhouse(ch_client.clone());
    let router = api::routes::build_router(state.clone());

    // 1. Create Chain
    let chain_response = post_json(
        &router,
        "/api/chains",
        json!({
            "chain_id": 31337,
            "name": "Local Pipeline E2E",
            "native_token_symbol": "ETH",
            "safe_confirmation_depth": 0,
            "default_max_block_window": 50,
            "rpc_notes": "full pipeline fixture"
        }),
    )
    .await?;
    assert_ok(chain_response, StatusCode::OK);

    // 2. Create RPC Endpoint
    let rpc_response = post_json(
        &router,
        "/api/rpc-endpoints",
        json!({ "chain_id": 31337, "url": rpc_url, "weight": 100 }),
    )
    .await?;
    assert_ok(rpc_response, StatusCode::OK);

    // 3. Create Subscription for CONTRACT_ADDRESS starting at block 100
    let sub_response = post_json(
        &router,
        "/api/subscriptions",
        json!({
            "chain_id": 31337,
            "contract_address": CONTRACT_ADDRESS,
            "start_block": 100,
            "realtime_enabled": true
        }),
    )
    .await?;
    assert_ok(sub_response.clone(), StatusCode::OK);

    // 4. Create Block Transaction Sync State starting at block 100
    let sync_config_res = request_json(
        &router,
        Method::PUT,
        "/api/chains/31337/block-transaction-sync",
        Some(json!({
            "start_block": 100,
            "end_block": 100,
            "batch_size": 1,
            "reorg_window": 0,
            "realtime_enabled": true,
            "status": "pending"
        })),
    )
    .await?;
    assert_ok(sync_config_res, StatusCode::OK);

    // 5. Trigger Raw Logs Collector
    collector::worker::collect_once(&state).await?;

    // Verify raw logs collected in ClickHouse
    let raw_logs_count = count_raw_logs_in_clickhouse(&ch_client, 31337, CONTRACT_ADDRESS).await?;
    assert_eq!(raw_logs_count, 1, "Expected 1 raw log in ClickHouse");

    // Query Raw Logs via REST Search DSL API
    let search_res = post_json(
        &router,
        "/api/raw-logs/search",
        json!({
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "contract_address", "operator": "eq", "value": CONTRACT_ADDRESS },
                { "field": "topic0", "operator": "eq", "value": TRANSFER_TOPIC0 }
            ],
            "sort": { "field": "block_number", "direction": "desc" }
        }),
    )
    .await?;
    assert_ok(search_res.clone(), StatusCode::OK);
    let found_logs = response_data(&search_res.1).as_array().expect("array of logs");
    assert_eq!(found_logs.len(), 1);
    assert_eq!(found_logs[0]["block_number"], 100);
    assert_eq!(found_logs[0]["contract_address"], CONTRACT_ADDRESS);

    // 6. Trigger Block Transaction Collector
    block_transaction::collector::collect_once(&state).await?;

    // Query Block via REST API
    let block_res = request_json(
        &router,
        Method::GET,
        "/api/chains/31337/blocks/100",
        None,
    )
    .await?;
    assert_ok(block_res.clone(), StatusCode::OK);
    assert_eq!(response_data(&block_res.1)["block_number"], 100);

    // Query Block Transactions via REST API
    let btx_res = request_json(
        &router,
        Method::GET,
        "/api/chains/31337/blocks/100/transactions",
        None,
    )
    .await?;
    assert_ok(btx_res.clone(), StatusCode::OK);
    let txs = response_data(&btx_res.1).as_array().expect("array of txs");
    assert_eq!(txs.len(), 1);
    let tx_hash = txs[0]["tx_hash"].as_str().expect("tx hash string");

    // Query Transaction Detail via REST API
    let tx_res = request_json(
        &router,
        Method::GET,
        &format!("/api/chains/31337/transactions/{tx_hash}"),
        None,
    )
    .await?;
    assert_ok(tx_res.clone(), StatusCode::OK);
    assert_eq!(response_data(&tx_res.1)["tx_hash"], tx_hash);

    // 7. Test Reorg Invalidation across both SQLite and ClickHouse
    let reorg_result = reorg::observe_block(
        &pool,
        31337,
        100,
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff1",
    )
    .await?;
    assert!(matches!(
        reorg_result,
        reorg::BlockCheckpointResult::ReorgDetected { .. }
    ));

    // Invalidate tombstones in ClickHouse
    clickhouse::invalidate_from_block(&ch_client, 31337, 100).await?;
    clickhouse::invalidate_blocks_and_transactions_from_block(&ch_client, 31337, 100).await?;

    // Verify Search DSL hides tombstoned logs
    let search_after_reorg = post_json(
        &router,
        "/api/raw-logs/search",
        json!({
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "contract_address", "operator": "eq", "value": CONTRACT_ADDRESS }
            ]
        }),
    )
    .await?;
    assert_ok(search_after_reorg.clone(), StatusCode::OK);
    assert_eq!(response_data(&search_after_reorg.1).as_array().unwrap().len(), 0);

    // Verify Block query returns 404 after Reorg
    let block_after_reorg = request_json(
        &router,
        Method::GET,
        "/api/chains/31337/blocks/100",
        None,
    )
    .await?;
    assert_error(block_after_reorg, StatusCode::NOT_FOUND);

    Ok(())
}
