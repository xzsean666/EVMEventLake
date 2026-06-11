use eventlake::{app::startup, configuration::ApplicationConfiguration, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let configuration = ApplicationConfiguration::from_environment()?;
    telemetry::initialize(&configuration.telemetry)?;

    startup::run(configuration).await
}
