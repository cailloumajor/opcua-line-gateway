use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use opcua::client::Client;
use opcua_line_gateway_config::OpcUaServerConfig;
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};

use crate::opcua::session::OpcUaSession;

// Session spawn retry time.
const SPAWN_RETRY_TIME: Duration = Duration::from_secs(5);

/// The maximum time allowed for a session to be connected.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Manages starting and stopping the sessions. Does these three things:
///
/// 1. Start the sessions
/// 2. Wait until shutdown
/// 3. Stop the sessions
#[instrument(name = "session_manager", skip_all)]
pub(crate) async fn run_session_manager(
    client: Arc<Client>,
    servers: BTreeMap<String, OpcUaServerConfig>,
    shutdown: CancellationToken,
) {
    info!(msg = "session manager started");

    // Store sessions spawn loop tasks handles to allow joining them when stopping.
    let mut session_spawn_handles = servers
        .into_iter()
        .map(|(id, config)| spawn_session(Arc::clone(&client), id, config))
        .collect::<JoinSet<_>>();

    shutdown.cancelled().await;

    session_spawn_handles.abort_all();
    while let Some(join_result) = session_spawn_handles.join_next().await {
        if let Ok(session) = join_result {
            // Ignore the result, as error logging is handled by the function `instrument` attribute.
            let _ = session.stop().await;
        }
    }

    info!(msg = "session manager terminated");
}

/// Utility function trying to spawn a session upon success.
async fn spawn_session(
    client: Arc<Client>,
    server_id: String,
    server_config: OpcUaServerConfig,
) -> OpcUaSession {
    let mut interval = interval(SPAWN_RETRY_TIME);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        interval.tick().await;
        let spawn_fut = OpcUaSession::spawn(
            Arc::clone(&client),
            server_id.clone(),
            server_config.clone(),
        );
        match timeout(CONNECT_TIMEOUT, spawn_fut).await {
            Ok(Ok(session)) => {
                break session;
            }
            Ok(Err(_)) => {}
            Err(_) => {
                error!(msg = "timeout spawning session", server_id);
            }
        }
    }
}
