use crate::configuration::TelemetryConfiguration;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn initialize(configuration: &TelemetryConfiguration) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_new(&configuration.log_level).or_else(|_| EnvFilter::try_new("info"))?;

    if configuration.json_logs {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .try_init()?;
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .try_init()?;
    }

    Ok(())
}
