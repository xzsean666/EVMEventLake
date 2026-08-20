use std::{env, net::SocketAddr, time::Duration};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
    routing::post,
};
use chrono::Utc;
use eventlake::{
    api, app::application_state::ApplicationState, auth, collector, configuration, database, reorg,
    rpc_pool, shared::hex::parse_hex_u64,
};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
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
async fn complete_eventlake_workflow_on_real_postgres() -> anyhow::Result<()> {
    let Some(database_url) = test_database_url() else {
        eprintln!("skipping real database e2e: .env.test DATABASE_URL is not configured");
        return Ok(());
    };

    let pool = PgPoolOptions::new()
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
    assert!(openapi_response.1["paths"].get("/api/search").is_some());
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
    assert!(
        openapi_response.1["paths"]
            .get("/api/explorer/events/{event_name}")
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

    let abi_response = post_json(
        &router,
        "/api/abis",
        json!({
            "name": "ERC20",
            "abi_json": erc20_transfer_abi()
        }),
    )
    .await?;
    assert_ok(abi_response.clone(), StatusCode::OK);
    let abi_id = uuid_from_response(&abi_response.1, "id");
    assert_eq!(response_data(&abi_response.1)["event_count"], 1);
    assert_ok(
        get(&router, &format!("/api/abis/{abi_id}")).await?,
        StatusCode::OK,
    );

    let events_response = get(&router, "/api/events").await?;
    assert_ok(events_response.clone(), StatusCode::OK);
    assert_eq!(
        response_data(&events_response.1)[0]["event_name"],
        "Transfer"
    );

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
    assert_eq!(count_rows(&pool, "eventlake_raw_logs").await?, 1);
    assert_eq!(count_rows(&pool, "eventlake_decode_queue").await?, 0);

    let raw_search = post_json(
        &router,
        "/api/raw-logs/search",
        json!({
            "page": 1,
            "limit": 10,
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "block_number", "operator": "eq", "value": 100 },
                { "field": "topic0", "operator": "eq", "value": TRANSFER_TOPIC0 }
            ],
            "sort": { "field": "block_number", "direction": "desc" }
        }),
    )
    .await?;
    assert_ok(raw_search.clone(), StatusCode::OK);
    assert_eq!(response_data(&raw_search.1).as_array().unwrap().len(), 1);
    assert_eq!(
        response_data(&raw_search.1)[0]["data"],
        uint256_topic_data(1234)
    );

    let dashboard = get(&router, "/api/dashboard").await?;
    assert_ok(dashboard.clone(), StatusCode::OK);
    assert_eq!(response_data(&dashboard.1)["total_raw_logs"], 1);
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
    assert_eq!(
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM eventlake_raw_logs WHERE removed = true")
            .fetch_one(&pool)
            .await?
            .0,
        1
    );
    let post_reorg_search = post_json(
        &router,
        "/api/raw-logs/search",
        json!({
            "page": 1,
            "limit": 10,
            "filters": [
                { "field": "chain_id", "operator": "eq", "value": 31337 },
                { "field": "topic0", "operator": "eq", "value": TRANSFER_TOPIC0 }
            ]
        }),
    )
    .await?;
    assert_ok(post_reorg_search.clone(), StatusCode::OK);
    assert_eq!(
        response_data(&post_reorg_search.1)
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let post_reorg_dashboard = get(&router, "/api/dashboard").await?;
    assert_ok(post_reorg_dashboard.clone(), StatusCode::OK);
    assert_eq!(response_data(&post_reorg_dashboard.1)["total_raw_logs"], 0);
    assert_eq!(
        response_data(&post_reorg_dashboard.1)["total_decoded_events"],
        0
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
        delete(&router, &format!("/api/subscriptions/{subscription_id}")).await?,
        StatusCode::OK,
    );
    assert_ok(
        delete(&router, &format!("/api/abis/{abi_id}")).await?,
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

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    reset_eventlake_namespace_before_migration(&pool).await?;
    database::migrate(&pool).await?;
    reset_eventlake_tables(&pool).await?;

    let state = build_test_state(database_url.clone(), pool.clone(), false);
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

    let abi_response = post_json(
        &router,
        "/api/abis",
        json!({
            "name": "Live ERC20",
            "abi_json": erc20_transfer_and_approval_abi()
        }),
    )
    .await?;
    assert_ok(abi_response.clone(), StatusCode::OK);
    let abi_id = uuid_from_response(&abi_response.1, "id");

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

    let raw_count = count_raw_logs_for_contract(&pool, BASE_CHAIN_ID, BASE_USDC_ADDRESS).await?;
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

async fn assert_authentication_modes(database_url: &str, pool: PgPool) -> anyhow::Result<()> {
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
            Some(BASE_USDC_ADDRESS),
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
}

fn build_test_state(
    database_url: String,
    pool: PgPool,
    require_authentication: bool,
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
            clickhouse: configuration::ClickHouseConfig {
                host: "localhost".to_owned(),
                port: 8123,
                user: "eventlake".to_owned(),
                password: "eventlake".to_owned(),
                database: "eventlake".to_owned(),
                enabled: false,
            },
            auth: configuration::AuthConfiguration {
                jwt_secret: "test-secret".to_owned(),
                require_authentication,
            },
            background: configuration::BackgroundConfiguration {
                workers_enabled: false,
                worker_tick: Duration::from_millis(50),
                decode_batch_size: 100,
                partition_tick: Duration::from_secs(300),
            },
            block_transaction: configuration::BlockTransactionConfiguration {
                enabled: false,
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

async fn reset_eventlake_tables(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        TRUNCATE TABLE
            eventlake_api_keys,
            eventlake_event_field_index,
            eventlake_address_index,
            eventlake_decoded_events,
            eventlake_decode_queue,
            eventlake_raw_logs,
            eventlake_block_checkpoints,
            eventlake_subscriptions,
            eventlake_contract_registry,
            eventlake_event_registry,
            eventlake_abi_versions,
            eventlake_rpc_endpoints
        RESTART IDENTITY CASCADE
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn reset_eventlake_namespace_before_migration(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        DROP TABLE IF EXISTS
            eventlake_api_keys,
            eventlake_event_field_index,
            eventlake_address_index,
            eventlake_decoded_events,
            eventlake_decode_queue,
            eventlake_raw_logs,
            eventlake_block_checkpoints,
            eventlake_subscriptions,
            eventlake_contract_registry,
            eventlake_event_registry,
            eventlake_abi_versions,
            eventlake_rpc_endpoints,
            eventlake_chains,
            eventlake_sqlx_migrations
        CASCADE
        "#,
    )
    .execute(pool)
    .await?;

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
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = request.get("id").cloned().unwrap_or_else(|| json!(1));

    let response = match method {
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
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "method not found" }
        }),
    };

    Json(response)
}

fn erc20_transfer_abi() -> Value {
    json!([
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "address", "name": "from", "type": "address"},
                {"indexed": true, "internalType": "address", "name": "to", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "value", "type": "uint256"}
            ],
            "name": "Transfer",
            "type": "event"
        }
    ])
}

fn erc20_transfer_and_approval_abi() -> Value {
    json!([
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "address", "name": "from", "type": "address"},
                {"indexed": true, "internalType": "address", "name": "to", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "value", "type": "uint256"}
            ],
            "name": "Transfer",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "address", "name": "owner", "type": "address"},
                {"indexed": true, "internalType": "address", "name": "spender", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "value", "type": "uint256"}
            ],
            "name": "Approval",
            "type": "event"
        }
    ])
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

async fn count_rows(pool: &PgPool, table_name: &'static str) -> anyhow::Result<i64> {
    let sql = format!("SELECT COUNT(*)::BIGINT FROM {table_name}");
    Ok(sqlx::query_as::<_, (i64,)>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await?
        .0)
}

async fn count_raw_logs_for_contract(
    pool: &PgPool,
    chain_id: i64,
    contract_address: &str,
) -> anyhow::Result<i64> {
    Ok(sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM eventlake_raw_logs
        WHERE chain_id = $1 AND contract_address = $2
        "#,
    )
    .bind(chain_id)
    .bind(contract_address)
    .fetch_one(pool)
    .await?
    .0)
}

#[tokio::test]
async fn block_transaction_sync_and_storage_guard_workflow() -> anyhow::Result<()> {
    let Some(database_url) = test_database_url() else {
        eprintln!("skipping real Postgres E2E: set DATABASE_URL or EVENTLAKE_DATABASE_URL");
        return Ok(());
    };

    let pool = PgPoolOptions::new()
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
    assert_ok(create_chain_response, StatusCode::CREATED);

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
