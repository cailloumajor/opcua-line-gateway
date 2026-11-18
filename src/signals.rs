use anyhow::Context as _;
use futures_util::StreamExt;
use futures_util::stream::SelectAll;
use reopen::Handle as ReopenHandle;
use strum::{Display as StrumDisplay, VariantArray};
use tokio::signal::unix::{SignalKind, signal};
use tokio_stream::wrappers::SignalStream;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, info_span};

/// Represents signals handled by this application.
#[derive(Clone, Copy, StrumDisplay, VariantArray)]
#[strum(prefix = "SIG", serialize_all = "UPPERCASE")]
enum HandledSignals {
    Term,
    Quit,
    Int,
    Hup,
}

impl HandledSignals {
    /// Return the [`SignalKind`] corresponding with this handled signal variant.
    fn signal_kind(&self) -> SignalKind {
        match self {
            Self::Term => SignalKind::terminate(),
            Self::Quit => SignalKind::quit(),
            Self::Int => SignalKind::interrupt(),
            Self::Hup => SignalKind::hangup(),
        }
    }
}

/// Register signals to be handled and spawn the task to handle them, provided:
///
/// * the [`CancellationToken`] to shutdown the application;
/// * the [`ReopenHandle`] to trigger log file reopening.
pub fn spawn_signals_handler(
    shutdown_token: CancellationToken,
    log_reopen_handle: ReopenHandle,
) -> anyhow::Result<()> {
    let mut signals_stream = HandledSignals::VARIANTS
        .iter()
        .map(|sig| {
            let listener = signal(sig.signal_kind())
                .with_context(|| format!("Failed to register listener for signal {sig}"))?;
            let signal_stream = SignalStream::new(listener).map(|_| *sig);
            Ok(signal_stream)
        })
        .collect::<Result<SelectAll<_>, anyhow::Error>>()?;

    tokio::spawn(
        async move {
            info!(msg = "started signals handler");

            loop {
                let signal = signals_stream
                    .next()
                    .await
                    .expect("signal listener should not produce None value");

                match signal {
                    HandledSignals::Term | HandledSignals::Quit | HandledSignals::Int => {
                        info!(msg="received termination signal, shutting down", %signal);
                        shutdown_token.cancel();
                    }
                    HandledSignals::Hup => {
                        info!(msg = "received signal, reopening log file", %signal);
                        log_reopen_handle.reopen();
                    }
                }
            }
        }
        .instrument(info_span!("signals_handler")),
    );

    Ok(())
}
