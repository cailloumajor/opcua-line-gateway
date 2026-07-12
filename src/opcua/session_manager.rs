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

/// Manages starting and stopping the sessions. Runs forever until shutdown.
#[instrument(name = "session_manager", skip_all)]
pub(crate) async fn run_session_manager(
    client: Arc<Client>,
    servers: BTreeMap<String, OpcUaServerConfig>,
    shutdown: CancellationToken,
) {
    info!(msg = "session manager started");

    // Helper closure to create the session spawning future.
    let spawn_session_future = |server_id: String, server_config: OpcUaServerConfig| {
        let cloned_client = Arc::clone(&client);

        async move {
            let mut interval = interval(SPAWN_RETRY_TIME);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                interval.tick().await;
                let spawn_fut = OpcUaSession::spawn(
                    cloned_client.clone(),
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
    };

    // Store sessions spawn loop tasks handles to allow joining them when stopping.
    let mut session_spawn_handles = servers
        .into_iter()
        .map(|(id, config)| spawn_session_future(id, config))
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
