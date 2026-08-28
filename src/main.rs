use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt;
use opcua_line_gateway_config::LineGatewayConfig;
use redb::Database;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use self::opcua::{create_client, sessions_manager};
use self::timezone::{init_system_timezone, system_timezone};
use self::traceability::{TraceabilityCache, TraceabilityDatabase};

mod opcua;
mod timezone;
mod traceability;

/// Handle signals as they are caught.
#[instrument(skip_all)]
async fn handle_signals(signals: Signals, shutdown_token: CancellationToken) {
    info!(msg = "started signals handler");

    let mut signals_stream = signals.map(|s| signal_name(s).unwrap_or("unknown"));
    match signals_stream.next().await {
        Some(signal) => {
            info!(msg = "received signal, shutting down", signal)
        }
        None => {
            error!(msg = "signals stream exhausted, shutting down");
        }
    }
    shutdown_token.cancel();

    info!(msg = "signals handler terminated");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Get the configuration.
    let Some(config_path) = env::args_os().nth(1).map(PathBuf::from) else {
        return Err(anyhow!(
            "Failed to get configuration file path from first positional argument"
        ));
    };
    let config = LineGatewayConfig::from_toml_file(config_path)
        .context("Failed to get configuration from file")?;

    // Create or open the traceability cache database (redb).
    let traceability_cache_db = Database::create(&config.traceability.redb_file)
        .context("Failed to open traceability cache database file")?;
    let traceability_cache = Arc::new(TraceabilityCache::new(traceability_cache_db));

    // Initialize the cached system timezone.
    init_system_timezone().context("Failed to get the system timezone")?;

    // Initialize tracing.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        // Configure log levels using RUST_LOG environment variable.
        .with(EnvFilter::from_default_env())
        .init();

    // Ensure cached timezone.
    let tz = system_timezone();
    info!(msg = "got system timezone", ?tz);

    // Create traceability database client and fetch the general part sheet columns.
    let db_config = &config.traceability.database;
    let db_cache = Arc::clone(&traceability_cache);
    let traceability_database = TraceabilityDatabase::new(db_config, db_cache)
        .context("Failed to create traceability database client")?;

    // Create OPC-UA client.
    let client = create_client(&config).context("Failed to create OPC-UA client")?;

    let signals = Signals::new(TERM_SIGNALS).context("Failed to register termination signals")?;
    let signals_handle = signals.handle();
    let shutdown_token = CancellationToken::new();
    let signals_task = tokio::spawn(handle_signals(signals, shutdown_token.clone()));

    // Start part sheets draining task.
    let draining_task = traceability_database.drain_part_sheets_task(
        config.traceability.database.part_sheets_drain_period,
        shutdown_token.clone(),
    );

    // Start the sessions manager, acting as the main program loop.
    sessions_manager(
        client.into(),
        config.machines,
        shutdown_token,
        traceability_cache,
    )
    .await;

    signals_handle.close();

    tokio::try_join!(draining_task, signals_task).context("failed to join tasks")?;

    Ok(())
}
