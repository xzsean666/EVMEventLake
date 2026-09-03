use std::net::SocketAddr;

use axum::http::HeaderValue;
use tokio::net::TcpListener;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    api, app::application_state::ApplicationState, background,
    configuration::ApplicationConfiguration, database,
};

const DEFAULT_JWT_SECRET: &str = "change-me";

pub async fn run(configuration: ApplicationConfiguration) -> anyhow::Result<()> {
    validate_security(&configuration)?;
    validate_storage_mode(&configuration)?;

    let pool = database::connect(&configuration.database).await?;
    database::migrate(&pool).await?;

    if let Some(ref seeds_path) = configuration.rpc_pool.seeds_path {
        let seed_result = crate::rpc_pool::seed_rpc_endpoints_from_file(&pool, seeds_path).await;
        if let Err(error) = seed_result {
            tracing::warn!(error = %error, path = %seeds_path, "failed to seed rpc endpoints from file");
        }
    }

    let address = SocketAddr::new(configuration.http.host, configuration.http.port);
    let cors = build_cors_layer(&configuration.http.cors_allowed_origins);
    let state = ApplicationState::new(configuration, pool);

    #[cfg(feature = "clickhouse")]
    let state = match crate::clickhouse::connect(&state.configuration.clickhouse).await {
        Ok(Some(client)) => state.with_clickhouse(client),
        Ok(None) => state,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "ClickHouse unavailable at startup; raw-log collection will retry until it recovers"
            );
            state
        }
    };

    background::spawn_workers(state.clone());

    let router = api::routes::build_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, "eventlake listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn validate_storage_mode(configuration: &ApplicationConfiguration) -> anyhow::Result<()> {
    #[cfg(feature = "clickhouse")]
    if configuration.block_transaction.enabled && !configuration.clickhouse.enabled {
        anyhow::bail!(
            "EVENTLAKE_BLOCK_TRANSACTION_ENABLED=true requires EVENTLAKE_CLICKHOUSE_ENABLED=true"
        );
    }

    #[cfg(not(feature = "clickhouse"))]
    {
        if configuration.clickhouse.enabled {
            anyhow::bail!(
                "EVENTLAKE_CLICKHOUSE_ENABLED=true requires a binary built with --features clickhouse"
            );
        }
        if configuration.block_transaction.enabled {
            anyhow::bail!(
                "EVENTLAKE_BLOCK_TRANSACTION_ENABLED=true requires a binary built with --features clickhouse"
            );
        }
    }

    Ok(())
}

/// Guards against shipping insecure auth defaults: a default JWT secret with auth on is a
/// hard error, and running with auth disabled is allowed but loudly warned about.
fn validate_security(configuration: &ApplicationConfiguration) -> anyhow::Result<()> {
    if configuration.auth.require_authentication {
        if configuration.auth.jwt_secret == DEFAULT_JWT_SECRET {
            anyhow::bail!(
                "EVENTLAKE_JWT_SECRET is still the default value; set a strong secret when EVENTLAKE_REQUIRE_AUTHENTICATION=true"
            );
        }
    } else {
        tracing::warn!(
            "authentication is DISABLED (EVENTLAKE_REQUIRE_AUTHENTICATION=false): every request is treated as admin"
        );
    }

    Ok(())
}

/// An empty allowlist means permissive CORS (convenient for local development); otherwise
/// only the configured origins are accepted.
fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    if allowed_origins.is_empty() {
        tracing::warn!("CORS is permissive (no EVENTLAKE_CORS_ALLOWED_ORIGINS configured)");
        return CorsLayer::permissive();
    }

    let origins: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|origin| match origin.parse::<HeaderValue>() {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::warn!(origin = %origin, "ignoring invalid CORS origin");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any)
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
