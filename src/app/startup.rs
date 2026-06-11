use std::net::SocketAddr;

use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    api, app::application_state::ApplicationState, background,
    configuration::ApplicationConfiguration, database,
};

pub async fn run(configuration: ApplicationConfiguration) -> anyhow::Result<()> {
    let pool = database::connect(&configuration.database).await?;
    database::migrate(&pool).await?;

    let address = SocketAddr::new(configuration.http.host, configuration.http.port);
    let state = ApplicationState::new(configuration, pool);

    background::spawn_workers(state.clone());

    let router = api::routes::build_router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, "eventlake listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate signal handler");
        signal.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
