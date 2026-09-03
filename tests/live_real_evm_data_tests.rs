#![cfg(feature = "clickhouse")]

use std::time::Duration;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use eventlake::{
    api,
    app::application_state::ApplicationState,
    clickhouse,
    configuration::{
        ApplicationConfiguration, AuthConfiguration, BackgroundConfiguration,
        BlockTransactionConfiguration, ClickHouseConfig, DatabaseConfiguration, HttpConfiguration,
        TelemetryConfiguration,
    },
    rpc_pool::evm_rpc_client,
};
use reqwest::Client;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const ETH_RPC_URLS: &[&str] = &[
    "https://ethereum-rpc.publicnode.com",
    "https://rpc.ankr.com/eth",
    "https://1rpc.io/eth",
    "https://eth.llamarpc.com",
    "https://cloudflare-eth.com",
];

const BASE_RPC_URLS: &[&str] = &[
    "https://mainnet.base.org",
    "https://base-rpc.publicnode.com",
    "https://1rpc.io/base",
];

async fn find_working_rpc(urls: &[&str], http_client: &Client) -> Option<String> {
    for &url in urls {
        match evm_rpc_client::eth_block_number(http_client, url).await {
            Ok(head) if head > 0 => {
                println!("Connected to RPC {url}, head: {head}");
                return Some(url.to_owned());
            }
            Err(err) => {
                println!("RPC {url} failed: {err}");
            }
            _ => {}
        }
    }
    None
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
        },
        block_transaction: BlockTransactionConfiguration {
            enabled: true,
            batch_size: 10,
            max_concurrency: 2,
            reorg_window: 32,
            max_response_bytes: 67108864,
        },
        rpc_pool: Default::default(),
        telemetry: TelemetryConfiguration {
            log_level: "info".to_owned(),
            json_logs: false,
        },
    }
}

#[tokio::test]
async fn test_live_ethereum_blocks_and_transactions() -> anyhow::Result<()> {
    let http_client = Client::builder().timeout(Duration::from_secs(15)).build()?;

    let eth_rpc = match find_working_rpc(ETH_RPC_URLS, &http_client).await {
        Some(url) => url,
        None => {
            eprintln!("Skipping live Ethereum test: no reachable public Ethereum RPC");
            return Ok(());
        }
    };

    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };

    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => {
            eprintln!("Skipping ClickHouse test: local ClickHouse not reachable on 8123");
            return Ok(());
        }
    };

    // 1. Fetch live Ethereum latest block
    let head = evm_rpc_client::eth_block_number(&http_client, &eth_rpc).await?;
    println!("Live Ethereum head block: {head}");
    assert!(head > 20_000_000, "Ethereum head should be > 20M");

    // Fetch batch of 3 recent safe blocks
    let test_start_block = head - 50;
    let block_numbers: Vec<i64> = (test_start_block..test_start_block + 3).collect();
    println!("Fetching batch of Ethereum blocks: {block_numbers:?}");

    let decoded_blocks =
        evm_rpc_client::eth_get_blocks_by_number_batch(&http_client, &eth_rpc, 1, &block_numbers)
            .await?;

    assert_eq!(decoded_blocks.len(), 3);
    for block in &decoded_blocks {
        println!(
            "Decoded Block {}: hash={}, txs={}, gas_used={}/{}, base_fee={:?}, blob_gas={:?}",
            block.block_number,
            block.block_hash,
            block.transaction_count,
            block.gas_used,
            block.gas_limit,
            block.base_fee_per_gas,
            block.blob_gas_used,
        );
        assert!(!block.block_hash.is_empty());
        assert!(!block.parent_hash.is_empty());
        assert!(block.timestamp > 0);
        assert!(!block.gas_limit.is_empty());
        assert!(!block.gas_used.is_empty());
    }

    // Verify block sequence
    evm_rpc_client::validate_block_sequence(&decoded_blocks)?;

    // 2. Write to ClickHouse
    clickhouse::write_blocks_and_transactions(&ch_client, &decoded_blocks).await?;
    println!("Successfully wrote 3 live Ethereum blocks to ClickHouse");

    // 3. Test API Queries
    let state = ApplicationState::new(test_configuration(ch_config.clone()), lazy_pool()?)
        .with_clickhouse(ch_client.clone());
    let router = api::routes::build_router(state);

    let target_block = &decoded_blocks[0];

    // Query Block Detail by Number
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/1/blocks/{}",
            target_block.block_number
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"]["block_number"], target_block.block_number);
    assert_eq!(json["data"]["block_hash"], target_block.block_hash);
    assert_eq!(
        json["data"]["transaction_count"],
        target_block.transaction_count
    );

    // Query Block Detail by Hash
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/chains/1/blocks/{}", target_block.block_hash))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);

    // Query Block Transactions with Pagination
    if target_block.transaction_count > 0 {
        let req = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/chains/1/blocks/{}/transactions?limit=2",
                target_block.block_number
            ))
            .body(Body::empty())?;
        let resp = router.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
        let json: Value = serde_json::from_slice(&bytes)?;
        let txs = json["data"].as_array().expect("data is array");
        assert!(!txs.is_empty());
        assert!(txs.len() <= 2);

        let first_tx = &target_block.transactions[0];
        println!("Checking first tx detail: {}", first_tx.tx_hash);

        // Query Transaction by Hash
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/chains/1/transactions/{}", first_tx.tx_hash))
            .body(Body::empty())?;
        let resp = router.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
        let tx_json: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(tx_json["data"]["tx_hash"], first_tx.tx_hash);
        assert_eq!(tx_json["data"]["from_address"], first_tx.from_address);
        assert_eq!(tx_json["data"]["value"], first_tx.value);
        assert_eq!(tx_json["data"]["nonce"], first_tx.nonce);

        // Query Address Transactions (from_address)
        let req = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/chains/1/addresses/{}/transactions?direction=from",
                first_tx.from_address
            ))
            .body(Body::empty())?;
        let resp = router.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
        let addr_json: Value = serde_json::from_slice(&bytes)?;
        let addr_txs = addr_json["data"].as_array().expect("data is array");
        assert!(!addr_txs.is_empty());
    }

    Ok(())
}

#[tokio::test]
async fn test_live_dencun_blob_and_contract_creation_blocks() -> anyhow::Result<()> {
    let http_client = Client::builder().timeout(Duration::from_secs(15)).build()?;

    let eth_rpc = match find_working_rpc(ETH_RPC_URLS, &http_client).await {
        Some(url) => url,
        None => return Ok(()),
    };

    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };

    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => return Ok(()),
    };

    let head = evm_rpc_client::eth_block_number(&http_client, &eth_rpc).await?;
    println!("Scanning recent blocks near head {head} for blob tx and contract creation...");

    let scan_range: Vec<i64> = (head - 20..head).collect();
    let blocks =
        evm_rpc_client::eth_get_blocks_by_number_batch(&http_client, &eth_rpc, 1, &scan_range)
            .await?;

    let mut found_contract_creation = false;
    let mut found_blob_tx = false;

    for block in &blocks {
        for tx in &block.transactions {
            if tx.to_address.is_none() {
                println!(
                    "Found live contract creation in block {}: tx_hash={}, method_id={:?}",
                    block.block_number, tx.tx_hash, tx.method_id
                );
                found_contract_creation = true;
            }
            if tx.tx_type == Some(3) {
                println!(
                    "Found live EIP-4844 blob tx in block {}: tx_hash={}, max_fee={:?}",
                    block.block_number, tx.tx_hash, tx.max_fee_per_gas
                );
                found_blob_tx = true;
            }
        }
    }

    println!(
        "Scan summary: found_contract_creation={}, found_blob_tx={}",
        found_contract_creation, found_blob_tx
    );

    // Write all scanned blocks to ClickHouse
    clickhouse::write_blocks_and_transactions(&ch_client, &blocks).await?;
    println!(
        "Successfully wrote {} live blocks to ClickHouse",
        blocks.len()
    );

    // Verify querying contract creation tx if present
    for block in &blocks {
        for tx in &block.transactions {
            if tx.to_address.is_none() {
                let stored =
                    clickhouse::get_transaction_by_hash(&ch_client, 1, &tx.tx_hash).await?;
                assert!(stored.is_some());
                let row = stored.unwrap();
                assert_eq!(row.to_address, None);
                assert_eq!(row.tx_hash, tx.tx_hash);
                break;
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_live_base_l2_blocks() -> anyhow::Result<()> {
    let http_client = Client::builder().timeout(Duration::from_secs(15)).build()?;

    let base_rpc = match find_working_rpc(BASE_RPC_URLS, &http_client).await {
        Some(url) => url,
        None => {
            eprintln!("Skipping Base test: no reachable public Base RPC");
            return Ok(());
        }
    };

    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };

    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => return Ok(()),
    };

    // Chain 8453 (Base)
    let head = evm_rpc_client::eth_block_number(&http_client, &base_rpc).await?;
    println!("Base head: {head}");

    let block =
        evm_rpc_client::eth_get_block_by_number(&http_client, &base_rpc, 8453, head - 10).await?;
    assert!(block.is_some());
    let b = block.unwrap();
    println!(
        "Base block {}: txs={}, hash={}",
        b.block_number, b.transaction_count, b.block_hash
    );

    clickhouse::write_blocks_and_transactions(&ch_client, std::slice::from_ref(&b)).await?;

    let fetched = clickhouse::get_block_by_number(&ch_client, 8453, b.block_number).await?;
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().block_hash, b.block_hash);

    Ok(())
}

#[tokio::test]
async fn test_keyset_cursor_pagination_and_tamper_proofing() -> anyhow::Result<()> {
    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };

    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => return Ok(()),
    };

    let chain_id = 77777;
    let test_addr = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // Generate 5 mock blocks with 3 transactions each for test_addr
    let mut mock_blocks = Vec::new();
    for bn in 1..=5 {
        let mut txs = Vec::new();
        for idx in 0..3 {
            txs.push(evm_rpc_client::DecodedTransaction {
                chain_id,
                tx_hash: format!("0x{:064x}", bn * 100 + idx),
                block_number: bn,
                transaction_index: idx,
                from_address: test_addr.to_owned(),
                to_address: Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()),
                value: "1000".to_owned(),
                nonce: format!("{idx}"),
                gas: "21000".to_owned(),
                gas_price: Some("1000000000".to_owned()),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                tx_type: Some(0),
                method_id: None,
            });
        }
        mock_blocks.push(evm_rpc_client::DecodedBlock {
            chain_id,
            block_number: bn,
            block_hash: format!("0x{:064x}", bn),
            parent_hash: format!("0x{:064x}", bn - 1),
            timestamp: 1700000000 + bn,
            gas_limit: "30000000".to_owned(),
            gas_used: "63000".to_owned(),
            base_fee_per_gas: None,
            beneficiary: None,
            transactions_root: None,
            receipts_root: None,
            state_root: None,
            size: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            transaction_count: 3,
            transactions: txs,
        });
    }

    clickhouse::write_blocks_and_transactions(&ch_client, &mock_blocks).await?;

    let state = ApplicationState::new(test_configuration(ch_config.clone()), lazy_pool()?)
        .with_clickhouse(ch_client.clone());
    let router = api::routes::build_router(state);

    // 1. Test Block Transactions Cursor Pagination (limit = 2 on block with 3 txs)
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/{chain_id}/blocks/1/transactions?limit=2"
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let page1_json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(page1_json["data"].as_array().unwrap().len(), 2);
    assert_eq!(page1_json["meta"]["has_more"], true);
    let next_cursor = page1_json["meta"]["next_cursor"].as_str().unwrap();

    // Page 2
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/{chain_id}/blocks/1/transactions?limit=2&cursor={next_cursor}"
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let page2_json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(page2_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(page2_json["meta"]["has_more"], false);
    assert!(page2_json["meta"]["next_cursor"].is_null());

    // 2. Test Address Transactions Keyset Cursor Pagination across 15 txs with limit = 4
    let mut all_fetched_hashes = Vec::new();
    let mut current_cursor: Option<String> = None;

    loop {
        let uri = if let Some(ref c) = current_cursor {
            format!(
                "/api/chains/{chain_id}/addresses/{test_addr}/transactions?direction=from&limit=4&cursor={c}"
            )
        } else {
            format!(
                "/api/chains/{chain_id}/addresses/{test_addr}/transactions?direction=from&limit=4"
            )
        };

        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())?;
        let resp = router.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
        let page: Value = serde_json::from_slice(&bytes)?;
        let data = page["data"].as_array().unwrap();
        for item in data {
            all_fetched_hashes.push(item["tx_hash"].as_str().unwrap().to_owned());
        }

        if page["meta"]["has_more"].as_bool() == Some(true) {
            current_cursor = page["meta"]["next_cursor"].as_str().map(|s| s.to_owned());
        } else {
            break;
        }
    }

    assert_eq!(
        all_fetched_hashes.len(),
        15,
        "Should have paged through all 15 transactions"
    );

    // 3. Test Cursor Tampering (Mismatched address/direction)
    let fake_cursor = next_cursor; // This was a block transaction cursor, not address cursor
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/chains/{chain_id}/addresses/{test_addr}/transactions?direction=from&cursor={fake_cursor}"
        ))
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    Ok(())
}

#[tokio::test]
async fn test_reorg_tombstone_and_reingest_recovery() -> anyhow::Result<()> {
    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };

    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => return Ok(()),
    };

    let chain_id = 88888;
    let original_block = evm_rpc_client::DecodedBlock {
        chain_id,
        block_number: 50,
        block_hash: "0x0000000000000000000000000000000000000000000000000000000000000050".to_owned(),
        parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000049"
            .to_owned(),
        timestamp: 1700000050,
        gas_limit: "30000000".to_owned(),
        gas_used: "21000".to_owned(),
        base_fee_per_gas: None,
        beneficiary: None,
        transactions_root: None,
        receipts_root: None,
        state_root: None,
        size: None,
        withdrawals_root: None,
        blob_gas_used: None,
        excess_blob_gas: None,
        parent_beacon_block_root: None,
        transaction_count: 1,
        transactions: vec![evm_rpc_client::DecodedTransaction {
            chain_id,
            tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000051"
                .to_owned(),
            block_number: 50,
            transaction_index: 0,
            from_address: "0x1111111111111111111111111111111111111111".to_owned(),
            to_address: Some("0x2222222222222222222222222222222222222222".to_owned()),
            value: "100".to_owned(),
            nonce: "0".to_owned(),
            gas: "21000".to_owned(),
            gas_price: Some("1000000000".to_owned()),
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            tx_type: Some(0),
            method_id: None,
        }],
    };

    // Ingest original block
    clickhouse::write_blocks_and_transactions(&ch_client, std::slice::from_ref(&original_block))
        .await?;
    let found = clickhouse::get_block_by_number(&ch_client, chain_id, 50).await?;
    assert!(found.is_some());

    // Invalidate due to reorg
    clickhouse::invalidate_blocks_and_transactions_from_block(&ch_client, chain_id, 50).await?;
    let after_invalidation = clickhouse::get_block_by_number(&ch_client, chain_id, 50).await?;
    assert!(
        after_invalidation.is_none(),
        "Invalidated block must not be returned"
    );

    let tx_after_invalidation = clickhouse::get_transaction_by_hash(
        &ch_client,
        chain_id,
        "0x0000000000000000000000000000000000000000000000000000000000000051",
    )
    .await?;
    assert!(
        tx_after_invalidation.is_none(),
        "Invalidated tx must not be returned"
    );

    // Re-ingest new canonical block (e.g. fork won)
    let mut reorged_block = original_block;
    reorged_block.block_hash =
        "0x0000000000000000000000000000000000000000000000000000000000000050_fork"
            .replace("_fork", "00");
    reorged_block.block_hash =
        "0x00000000000000000000000000000000000000000000000000000000000000ff".to_owned();

    clickhouse::write_blocks_and_transactions(&ch_client, std::slice::from_ref(&reorged_block))
        .await?;

    let restored = clickhouse::get_block_by_number(&ch_client, chain_id, 50).await?;
    assert!(restored.is_some());
    assert_eq!(
        restored.unwrap().block_hash,
        "0x00000000000000000000000000000000000000000000000000000000000000ff"
    );

    Ok(())
}

#[tokio::test]
async fn test_api_error_responses_400_404_503() -> anyhow::Result<()> {
    let mut ch_disabled_config = test_configuration(ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: false,
    });
    ch_disabled_config.block_transaction.enabled = false;

    // Test with ClickHouse disabled -> 503
    let disabled_state = ApplicationState::new(ch_disabled_config, lazy_pool()?);
    let disabled_router = api::routes::build_router(disabled_state);

    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/1/blocks/100")
        .body(Body::empty())?;
    let resp = disabled_router.oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    // Test with ClickHouse enabled
    let ch_config = ClickHouseConfig {
        host: "127.0.0.1".to_owned(),
        port: 8123,
        user: "eventlake".to_owned(),
        password: "eventlake".to_owned(),
        database: "eventlake".to_owned(),
        enabled: true,
    };
    let ch_client = match clickhouse::connect(&ch_config).await {
        Ok(Some(c)) => c,
        _ => return Ok(()),
    };

    let state = ApplicationState::new(test_configuration(ch_config), lazy_pool()?)
        .with_clickhouse(ch_client);
    let router = api::routes::build_router(state);

    // 400 Bad Request on negative chain_id
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/-1/blocks/100")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 400 Bad Request on invalid block_ref
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/1/blocks/not-a-number")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 400 Bad Request on invalid address
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/1/addresses/0xshort/transactions")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 400 Bad Request on invalid direction
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/1/addresses/0x0000000000000000000000000000000000000001/transactions?direction=invalid")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 404 Not Found on non-existent block
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/9999999/blocks/999999999")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 404 Not Found on non-existent transaction
    let req = Request::builder()
        .method("GET")
        .uri("/api/chains/9999999/transactions/0x0000000000000000000000000000000000000000000000000000000000000001")
        .body(Body::empty())?;
    let resp = router.clone().oneshot(req).await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    Ok(())
}
