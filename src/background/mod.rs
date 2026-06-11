use crate::{app::application_state::ApplicationState, collector, decoder, indexing, rpc_pool};

pub fn spawn_workers(state: ApplicationState) {
    if !state.configuration.background.workers_enabled {
        tracing::info!("background workers disabled");
        return;
    }

    tokio::spawn(rpc_pool::worker::run(state.clone()));
    tokio::spawn(collector::worker::run(state.clone()));
    tokio::spawn(decoder::worker::run(state.clone()));
    tokio::spawn(indexing::partition_manager::run(state));
}
