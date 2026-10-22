use std::ffi::OsStr;
use std::path::PathBuf;
use std::{env, io};

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt;
use jiff::tz::TimeZone;
use logroller::{Compression, LogRollerBuilder, Rotation, RotationAge};
use opcua_line_gateway::{TimeProvider, run_main};
use opcua_line_gateway_config::{Credentials, LineGatewayConfig};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tick::SimpleClock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::EnvFilter;

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

    let _log_worker_guard = init_logging().context("Failed to initialize logging")?;

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
fn init_logging() -> anyhow::Result<WorkerGuard> {
    let logs_dir = env::var_os("LOGS_DIRECTORY");

    // Build the nonblocking tracing appender.
    let (non_blocking, guard) = match logs_dir.as_ref() {
        Some(dir) => {
            eprintln!(
                "LOGS_DIRECTORY env var is set, will log to {}",
                dir.display()
            );

            let appender = LogRollerBuilder::new(dir.as_os_str(), OsStr::new("%Y-%m-%d.log"))
                .time_zone(logroller::TimeZone::UTC)
                .rotation(Rotation::AgeBased(RotationAge::Daily))
                .compression(Compression::Gzip)
                .max_keep_files(365)
                .file_mode(0o644)
                .build()
                .context("Failed to build rolling logs manager")?;
            NonBlockingBuilder::default().lossy(false).finish(appender)
        }
        None => {
            eprintln!("LOGS_DIRECTORY env var not set, will log to stderr");

            NonBlockingBuilder::default()
                .lossy(false)
                .finish(io::stderr())
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(non_blocking)
        .with_ansi(logs_dir.is_none())
        .try_init()
        .map_err(anyhow::Error::from_boxed)
        .context("Failed to initialize subscriber")?;

    Ok(guard)
}
