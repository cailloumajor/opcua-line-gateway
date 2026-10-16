use std::env;
use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt;
use jiff::tz::TimeZone;
use opcua_line_gateway::{TimeProvider, run_main};
use opcua_line_gateway_config::{Credentials, LineGatewayConfig};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tick::SimpleClock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

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

    let credentials = Credentials::from_credentials_file().context("Failed to get credentials")?;

    init_logging().context("Failed to initialize logging")?;

    let tz = TimeZone::try_system().context("Failed to get system timezone")?;
    info!(msg = "got system timezone", ?tz);

    let time_provider = TimeProvider::new(SimpleClock::new_system(), tz);

    let shutdown_token = CancellationToken::new();

    let signals = Signals::new(TERM_SIGNALS).context("Failed to register termination signals")?;
    let signals_handle = signals.handle();
    let signals_task = tokio::spawn(handle_signals(signals, shutdown_token.clone()));

    run_main(config, credentials, time_provider, shutdown_token).await?;

    signals_handle.close();
    signals_task.await.context("failed to join signals task")?;

    Ok(())
}

/// Initializes logging system.
fn init_logging() -> anyhow::Result<()> {
    let env_filter = EnvFilter::from_default_env();

    // `JOURNAL_STREAM` is set by systemd when stdout/stderr are connected to the
    // journal, making it a good clue to know if we are ran by systemd.
    let journald = std::env::var_os("JOURNAL_STREAM")
        .map(|_| tracing_journald::layer())
        .transpose()
        .context("Failed to build journald layer")?;

    // Console output fallback.
    let console = journald.is_none().then(fmt::layer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(journald)
        .with(console)
        .try_init()
        .context("Failed to initialize subscriber")
}
