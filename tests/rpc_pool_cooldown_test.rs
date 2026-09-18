use chrono::{Duration, Utc};
use uuid::Uuid;

use eventlake::rpc_pool::{
    calculate_cooldown_seconds, get_cooldown_info, get_endpoint_runtime_status,
    reset_endpoint_cooldown_for_test, RpcEndpointRecord,
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
    reset_endpoint_cooldown_for_test();
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
    assert!(val["cooldown_until"].is_string());
}
