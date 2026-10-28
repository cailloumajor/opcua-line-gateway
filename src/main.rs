use std::fs::{OpenOptions, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{env, io};

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt;
use jiff::tz::TimeZone;
use opcua_line_gateway::{TimeProvider, run_main};
use opcua_line_gateway_config::{Credentials, LineGatewayConfig};
use reopen::{Handle as ReopenHandle, Reopen};
use signal_hook::consts::{SIGHUP, TERM_SIGNALS};
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tick::SimpleClock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::EnvFilter;

/// Log file name.
const LOG_FILE: &str = concat!(env!("CARGO_PKG_NAME"), ".log");

/// Handle signals as they are caught.
#[instrument(skip_all)]
async fn handle_signals(
    mut signals: Signals,
    shutdown_token: CancellationToken,
    log_reopen_handle: ReopenHandle,
) {
    info!(msg = "started signals handler");

    while let Some(signal) = signals.next().await {
        match signal {
            SIGHUP => {
                info!(msg = "received SIGHUP signal, triggering log file reopening");
                log_reopen_handle.reopen();
            }
            term_signal if TERM_SIGNALS.contains(&term_signal) => {
                let signal_name = signal_name(term_signal).unwrap_or("unknown");
                info!(
                    msg = "received termination signal, shutting down",
                    signal_name
                );
                shutdown_token.cancel();
            }
            _ => unreachable!(),
        }
    }

    // If we got here, we expect shutdown to have been triggered.
    if !shutdown_token.is_cancelled() {
        error!(msg = "signals stream exhausted unexpectedly, shutting down");
        shutdown_token.cancel();
    }

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

    let (_log_worker_guard, log_reopen_handle) =
        init_logging().context("Failed to initialize logging")?;

    let tz = TimeZone::try_system().context("Failed to get system timezone")?;
    info!(msg = "got system timezone", ?tz);

    let time_provider = TimeProvider::new(SimpleClock::new_system(), tz);

    let shutdown_token = CancellationToken::new();

    let mut handled_signals = vec![SIGHUP];
    handled_signals.extend(TERM_SIGNALS);
    let signals = Signals::new(handled_signals).context("Failed to register signals")?;
    let signals_handle = signals.handle();
    let signals_task = tokio::spawn(handle_signals(
        signals,
        shutdown_token.clone(),
        log_reopen_handle,
    ));

    run_main(config, credentials, time_provider, shutdown_token).await?;

    signals_handle.close();
    signals_task.await.context("failed to join signals task")?;

    Ok(())
}

/// Initializes logging system.
///
/// Returns a guard for the non-blocking log writer, and a handle for log file
/// reopening. If logging to the standard error, the reopen handle will have no
/// effect.
fn init_logging() -> anyhow::Result<(WorkerGuard, ReopenHandle)> {
    let logs_dir = env::var_os("LOGS_DIRECTORY");

    let reopen_handle = reopen::Handle::stub();

    // Build the nonblocking tracing appender.
    let (non_blocking, guard) = match logs_dir.as_ref() {
        Some(dir) => {
            let logfile_path = Path::new(dir).join(LOG_FILE);

            eprintln!(
                "LOGS_DIRECTORY env var is set, will log to {}",
                logfile_path.display()
            );

            let open_logfile = Box::new(move || {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&logfile_path)?;
                file.set_permissions(Permissions::from_mode(0o644))?;

                Ok(file)
            });
            let reopen = Reopen::with_handle(reopen_handle.clone(), open_logfile)
                .context("Failed to create log file reopening proxy")?;

            NonBlockingBuilder::default().lossy(false).finish(reopen)
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

    Ok((guard, reopen_handle))
}
