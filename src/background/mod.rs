use crate::{app::application_state::ApplicationState, block_transaction, collector, rpc_pool};

pub fn spawn_workers(state: ApplicationState) {
    if !state.configuration.background.workers_enabled {
        tracing::info!("background workers disabled");
        return;
    }

    if state.configuration.background.rpc_healthcheck_enabled {
        tokio::spawn(rpc_pool::worker::run(state.clone()));
    }
    if state.configuration.block_transaction.enabled {
        tokio::spawn(block_transaction::worker::run(state.clone()));
    }
    tokio::spawn(collector::worker::run(state));
}
