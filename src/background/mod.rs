use crate::{
    app::application_state::ApplicationState, block_transaction, collector, indexing, rpc_pool,
};

pub fn spawn_workers(state: ApplicationState) {
    if !state.configuration.background.workers_enabled {
        tracing::info!("background workers disabled");
        return;
    }

    tokio::spawn(rpc_pool::worker::run(state.clone()));
    if state.configuration.block_transaction.enabled {
        tokio::spawn(block_transaction::worker::run(state.clone()));
    }
    // EventLake is now a raw-event lake. ABI decoding is intentionally delegated to
    // downstream consumers, so no decode worker or decode queue is started here.
    tokio::spawn(async move {
        if let Err(error) = indexing::partition_manager::ensure_partitions(&state.pool).await {
            tracing::warn!(error = %error, "initial partition setup failed");
        }

        tokio::spawn(collector::worker::run(state.clone()));
        indexing::partition_manager::run(state).await;
    });
}
