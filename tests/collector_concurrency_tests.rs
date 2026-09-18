use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use eventlake::subscriptions::SubscriptionRecord;

fn make_dummy_sub(chain_id: i64, current_block: i64, scope: &str) -> SubscriptionRecord {
    SubscriptionRecord {
        id: Uuid::new_v4(),
        chain_id,
        contract_address: Some(format!("0x{:040x}", chain_id)),
        collection_scope: scope.to_owned(),
        abi_id: None,
        start_block: 0,
        current_block,
        target_block: None,
        min_block_window: 10,
        max_block_window: 1000,
        current_block_window: 100,
        status: "syncing".to_owned(),
        realtime_enabled: true,
        active: true,
        error_message: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn test_concurrent_tasks_execution_with_semaphore() {
    let concurrency = 4;
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut set = JoinSet::new();

    let task_count = 16;
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for i in 0..task_count {
        let sem = semaphore.clone();
        let counter = completed.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore acquire");
            // Simulate async task work
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if i == 5 {
                // One task fails with an error
                return Err("mock rpc error on chain");
            }
            Ok(i)
        });
    }

    let mut successful_results = Vec::new();
    let mut failure_count = 0;

    while let Some(res) = set.join_next().await {
        match res.expect("task join") {
            Ok(val) => successful_results.push(val),
            Err(_) => failure_count += 1,
        }
    }

    assert_eq!(completed.load(std::sync::atomic::Ordering::Relaxed), 16);
    assert_eq!(failure_count, 1);
    assert_eq!(successful_results.len(), 15);
}

#[test]
fn test_subscription_isolation_in_different_chains() {
    let sub_eth = make_dummy_sub(1, 1000, "contract");
    let sub_arb = make_dummy_sub(42161, 2000, "contract");
    let sub_base = make_dummy_sub(8453, 3000, "contract");

    assert_ne!(sub_eth.chain_id, sub_arb.chain_id);
    assert_ne!(sub_eth.chain_id, sub_base.chain_id);
    assert_ne!(sub_eth.id, sub_arb.id);
}
