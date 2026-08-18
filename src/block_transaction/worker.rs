use tokio::time::{MissedTickBehavior, interval};

use crate::{app::application_state::ApplicationState, block_transaction::collector};

pub async fn run(state: ApplicationState) {
    let mut ticker = interval(state.configuration.background.worker_tick);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(error) = collector::collect_once(&state).await {
            tracing::warn!(error = %error, "block-transaction worker tick failed");
        }
    }
}
