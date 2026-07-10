use std::env;
use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt;
use opcua_line_gateway_config::LineGatewayConfig;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::opcua::{SessionManager, create_client};

mod opcua;

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

    // Initialize tracing.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        // Configure log levels using RUST_LOG environment variable.
        .with(EnvFilter::from_default_env())
        .init();

    // Create OPC-UA client.
    let client = create_client(&config).context("Failed to create OPC-UA client")?;

    let session_manager = SessionManager::new(config.opcua_servers, client.into());

    let signals = Signals::new(TERM_SIGNALS).context("Failed to register termination signals")?;
    let signals_handle = signals.handle();
    let shutdown_token = CancellationToken::new();
    let signals_task = tokio::spawn(handle_signals(signals, shutdown_token.clone()));

    session_manager.run(shutdown_token).await;

    signals_handle.close();

    signals_task
        .await
        .context("Failed to join signals handling task")?;

    Ok(())
}
