use chrono::{Duration, Utc};
use uuid::Uuid;

use eventlake::rpc_pool::{
    calculate_cooldown_seconds, get_cooldown_info, get_endpoint_runtime_status,
    select_rpc_endpoint, select_weighted_round_robin, RpcEndpointRecord,
};

#[test]
fn test_calculate_cooldown_seconds_progression() {
    assert_eq!(calculate_cooldown_seconds(0), 0);
    assert_eq!(calculate_cooldown_seconds(1), 60); // 1 min
    assert_eq!(calculate_cooldown_seconds(2), 300); // 5 min
    assert_eq!(calculate_cooldown_seconds(3), 900); // 15 min
    assert_eq!(calculate_cooldown_seconds(4), 3600); // 1 hour
    assert_eq!(calculate_cooldown_seconds(5), 14400); // 4 hours
    assert_eq!(calculate_cooldown_seconds(6), 86400); // 24 hours (max)
    assert_eq!(calculate_cooldown_seconds(100), 86400); // capped at 24 hours
}

#[tokio::test]
async fn test_in_memory_cooldown_lifecycle_with_recovery() {
    let endpoint_id = Uuid::new_v4();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(50))
        .connect_lazy("sqlite::memory:")
        .expect("lazy connect");

    // Initially clean
    let (until, remaining) = get_cooldown_info(&endpoint_id, Utc::now());
    assert!(until.is_none());
    assert!(remaining.is_none());

    // 1st failure: 60s
    let _ = eventlake::rpc_pool::mark_rpc_failure(&pool, endpoint_id, "error 1").await;
    let status = get_endpoint_runtime_status(&endpoint_id);
    assert_eq!(status.consecutive_failures, 1);
    assert!(status.cooldown_until.is_some());
    let (_, remaining) = get_cooldown_info(&endpoint_id, Utc::now());
    assert!(remaining.is_some());
    let rem = remaining.unwrap();
    assert!(rem > 0 && rem <= 60);

    // 2nd failure: 300s (5m)
    let _ = eventlake::rpc_pool::mark_rpc_failure(&pool, endpoint_id, "error 2").await;
    let status = get_endpoint_runtime_status(&endpoint_id);
    assert_eq!(status.consecutive_failures, 2);
    let (_, remaining) = get_cooldown_info(&endpoint_id, Utc::now());
    assert!(remaining.is_some());
    let rem = remaining.unwrap();
    assert!(rem > 60 && rem <= 300);

    // 3rd failure: 900s (15m)
    let _ = eventlake::rpc_pool::mark_rpc_failure(&pool, endpoint_id, "error 3").await;
    let status = get_endpoint_runtime_status(&endpoint_id);
    assert_eq!(status.consecutive_failures, 3);
    let (_, remaining) = get_cooldown_info(&endpoint_id, Utc::now());
    let rem = remaining.unwrap();
    assert!(rem > 300 && rem <= 900);

    // Success recovery: clears failures and cooldown
    let _ = eventlake::rpc_pool::mark_rpc_success(&pool, endpoint_id).await;
    let status = get_endpoint_runtime_status(&endpoint_id);
    assert_eq!(status.consecutive_failures, 0);
    assert!(status.cooldown_until.is_none());
    let (until, remaining) = get_cooldown_info(&endpoint_id, Utc::now());
    assert!(until.is_none());
    assert!(remaining.is_none());
}

#[test]
fn test_rpc_endpoint_record_serialization_with_cooldown() {
    let now = Utc::now();
    let cd_until = now + Duration::seconds(300);
    let record = RpcEndpointRecord {
        id: Uuid::new_v4(),
        chain_id: 1,
        url: "https://rpc.ankr.com/eth".to_owned(),
        status: "healthy".to_owned(),
        weight: 100,
        latency_ms: Some(42),
        last_check_at: Some(now),
        failure_count: 2,
        last_error: Some("gateway timeout".to_owned()),
        is_archive: true,
        max_block_range: Some(5000),
        max_batch_size: Some(25),
        created_at: now,
        updated_at: now,
        cooldown_until: Some(cd_until),
        cooldown_remaining_seconds: Some(300),
    };

    let serialized = serde_json::to_string(&record).expect("serializes to json");
    let val: serde_json::Value = serde_json::from_str(&serialized).expect("deserializes");

    assert_eq!(val["status"], "healthy");
    assert_eq!(val["failure_count"], 2);
    assert_eq!(val["cooldown_remaining_seconds"], 300);
    assert_eq!(val["is_archive"], true);
    assert_eq!(val["max_block_range"], 5000);
    assert_eq!(val["max_batch_size"], 25);
    assert!(val["cooldown_until"].is_string());
}

fn create_mock_endpoint(id: Uuid, weight: i32) -> RpcEndpointRecord {
    let now = Utc::now();
    RpcEndpointRecord {
        id,
        chain_id: 1,
        url: format!("https://rpc-{id}.example.com"),
        status: "healthy".to_owned(),
        weight,
        latency_ms: Some(20),
        last_check_at: Some(now),
        failure_count: 0,
        last_error: None,
        is_archive: true,
        max_block_range: None,
        max_batch_size: None,
        created_at: now,
        updated_at: now,
        cooldown_until: None,
        cooldown_remaining_seconds: None,
    }
}

#[test]
fn test_smooth_weighted_round_robin_equal_weights() {
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let ep_a = create_mock_endpoint(id_a, 100);
    let ep_b = create_mock_endpoint(id_b, 100);
    let candidates = vec![ep_a, ep_b];

    let mut selected_ids = Vec::new();
    for _ in 0..10 {
        let selected = select_weighted_round_robin(&candidates);
        selected_ids.push(selected.id);
    }

    // Must alternate cleanly: A, B, A, B, A, B, A, B, A, B
    for i in 0..10 {
        if i % 2 == 0 {
            assert_eq!(selected_ids[i], id_a);
        } else {
            assert_eq!(selected_ids[i], id_b);
        }
    }
}

#[test]
fn test_smooth_weighted_round_robin_unequal_weights() {
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let ep_a = create_mock_endpoint(id_a, 4);
    let ep_b = create_mock_endpoint(id_b, 1);
    let candidates = vec![ep_a, ep_b];

    let mut selected_ids = Vec::new();
    for _ in 0..5 {
        let selected = select_weighted_round_robin(&candidates);
        selected_ids.push(selected.id);
    }

    // Classic SWRR for 4:1 produces: A, A, B, A, A
    assert_eq!(selected_ids, vec![id_a, id_a, id_b, id_a, id_a]);

    // Count after 50 selections should be exactly 40 for A and 10 for B
    let mut count_a = 0;
    let mut count_b = 0;
    for _ in 0..45 {
        let selected = select_weighted_round_robin(&candidates);
        if selected.id == id_a {
            count_a += 1;
        } else if selected.id == id_b {
            count_b += 1;
        }
    }
    // Total 50: count_a = 4 (from first 5) + 36, count_b = 1 + 9
    assert_eq!(count_a + 4, 40);
    assert_eq!(count_b + 1, 10);
}

#[tokio::test]
async fn test_select_rpc_endpoint_swrr_with_sqlite_and_cooldown() -> anyhow::Result<()> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    eventlake::database::migrate(&pool).await?;

    let chain_id = 12345;
    sqlx::query(
        "INSERT INTO eventlake_chains (chain_id, name, native_token_symbol) VALUES ($1, $2, $3)"
    )
    .bind(chain_id)
    .bind("TestChain")
    .bind("TEST")
    .execute(&pool)
    .await?;

    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    // Node A: weight 80
    sqlx::query(
        r#"
        INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, status, weight)
        VALUES ($1, $2, $3, 'healthy', 80)
        "#
    )
    .bind(id_a)
    .bind(chain_id)
    .bind("https://node-a.test")
    .execute(&pool)
    .await?;

    // Node B: weight 20
    sqlx::query(
        r#"
        INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, status, weight)
        VALUES ($1, $2, $3, 'healthy', 20)
        "#
    )
    .bind(id_b)
    .bind(chain_id)
    .bind("https://node-b.test")
    .execute(&pool)
    .await?;

    // Test 1: Both nodes healthy -> 10 requests should distribute 8 to A and 2 to B (ratio 4:1)
    let mut counts = std::collections::HashMap::new();
    for _ in 0..10 {
        let ep = select_rpc_endpoint(&pool, chain_id).await?;
        *counts.entry(ep.id).or_insert(0) += 1;
    }
    assert_eq!(counts.get(&id_a).copied().unwrap_or(0), 8);
    assert_eq!(counts.get(&id_b).copied().unwrap_or(0), 2);

    // Test 2: Node A fails -> enters cooldown
    eventlake::rpc_pool::mark_rpc_failure(&pool, id_a, "timeout error").await?;

    // Next 5 requests must all be served by Node B
    for _ in 0..5 {
        let ep = select_rpc_endpoint(&pool, chain_id).await?;
        assert_eq!(ep.id, id_b);
    }

    // Test 3: Node A recovers -> clears cooldown
    eventlake::rpc_pool::mark_rpc_success(&pool, id_a).await?;

    // Next 10 requests should resume SWRR sharing between A and B
    let mut resumed_counts = std::collections::HashMap::new();
    for _ in 0..10 {
        let ep = select_rpc_endpoint(&pool, chain_id).await?;
        *resumed_counts.entry(ep.id).or_insert(0) += 1;
    }
    assert!(resumed_counts.get(&id_a).copied().unwrap_or(0) > 0);
    assert!(resumed_counts.get(&id_b).copied().unwrap_or(0) > 0);

    Ok(())
}

#[tokio::test]
async fn test_select_rpc_endpoint_with_archive_and_range_requirements() -> anyhow::Result<()> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    eventlake::database::migrate(&pool).await?;

    let chain_id = 1868;
    sqlx::query(
        r#"
        INSERT INTO eventlake_chains (chain_id, name, native_token_symbol, safe_confirmation_depth, default_min_block_window, default_max_block_window)
        VALUES ($1, 'Soneium Test', 'ETH', 12, 1, 1000)
        "#
    )
    .bind(chain_id)
    .execute(&pool)
    .await?;

    let id_archive = Uuid::new_v4();
    let id_pruned = Uuid::new_v4();

    // Node 1: Archive node with 20000 max block range, weight 100
    sqlx::query(
        r#"
        INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, status, weight, is_archive, max_block_range, max_batch_size)
        VALUES ($1, $2, 'https://archive-node.test', 'healthy', 100, 1, 20000, 50)
        "#
    )
    .bind(id_archive)
    .bind(chain_id)
    .execute(&pool)
    .await?;

    // Node 2: Pruned node (is_archive = 0) with 1000 max block range, weight 100
    sqlx::query(
        r#"
        INSERT INTO eventlake_rpc_endpoints (id, chain_id, url, status, weight, is_archive, max_block_range, max_batch_size)
        VALUES ($1, $2, 'https://pruned-node.test', 'healthy', 100, 0, 1000, 20)
        "#
    )
    .bind(id_pruned)
    .bind(chain_id)
    .execute(&pool)
    .await?;

    // Test 1: Historical sync requires archive -> MUST strictly select Node 1 (Archive)
    let archive_req = eventlake::rpc_pool::EndpointRequirements {
        needs_archive: true,
        preferred_block_range: None,
    };
    for _ in 0..5 {
        let ep = eventlake::rpc_pool::select_rpc_endpoint_with_requirements(&pool, chain_id, &archive_req).await?;
        assert_eq!(ep.id, id_archive, "Historical queries must only route to archive nodes");
        assert!(ep.is_archive);
    }

    // Test 2: Realtime sync does not require archive -> Both nodes share traffic
    let realtime_req = eventlake::rpc_pool::EndpointRequirements {
        needs_archive: false,
        preferred_block_range: None,
    };
    let mut realtime_counts = std::collections::HashMap::new();
    for _ in 0..10 {
        let ep = eventlake::rpc_pool::select_rpc_endpoint_with_requirements(&pool, chain_id, &realtime_req).await?;
        *realtime_counts.entry(ep.id).or_insert(0) += 1;
    }
    assert!(realtime_counts.get(&id_archive).copied().unwrap_or(0) > 0);
    assert!(realtime_counts.get(&id_pruned).copied().unwrap_or(0) > 0);

    // Test 3: Large block range requested (5000) -> Prioritizes archive node (since pruned only supports 1000)
    let large_range_req = eventlake::rpc_pool::EndpointRequirements {
        needs_archive: false,
        preferred_block_range: Some(5000),
    };
    for _ in 0..5 {
        let ep = eventlake::rpc_pool::select_rpc_endpoint_with_requirements(&pool, chain_id, &large_range_req).await?;
        assert_eq!(ep.id, id_archive, "Large block range queries must prioritize nodes supporting large range");
    }

    Ok(())
}


