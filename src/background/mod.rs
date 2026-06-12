use crate::{app::application_state::ApplicationState, collector, decoder, indexing, rpc_pool};

pub fn spawn_workers(state: ApplicationState) {
    if !state.configuration.background.workers_enabled {
        tracing::info!("background workers disabled");
        return;
    }

    tokio::spawn(rpc_pool::worker::run(state.clone()));
    tokio::spawn(decoder::worker::run(state.clone()));
    tokio::spawn(async move {
        if let Err(error) = indexing::partition_manager::ensure_partitions(&state.pool).await {
            tracing::warn!(error = %error, "initial partition setup failed");
        }

        tokio::spawn(collector::worker::run(state.clone()));
        indexing::partition_manager::run(state).await;
    });
}
