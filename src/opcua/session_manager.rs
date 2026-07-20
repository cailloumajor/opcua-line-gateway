use std::collections::BTreeMap;
use std::mem;
use std::ops::DerefMut;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use opcua::client::Client;
use opcua_line_gateway_config::OpcUaServerConfig;
use parking_lot::Mutex;
use tokio::time::{MissedTickBehavior, interval, timeout};
use tokio_stream::wrappers::IntervalStream;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};

use crate::opcua::session::{OpcUaSession, start_session};

/// The interval for sessions management ticks.
const MANAGER_TICK_INTERVAL: Duration = Duration::from_secs(5);

/// The maximum time allowed for a session to be connected.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// OPC-UA sessions manager.
///
/// This will run forever until provided shutdown token triggers.
#[instrument(skip_all)]
pub(crate) async fn sessions_manager(
    client: Arc<Client>,
    servers: BTreeMap<String, OpcUaServerConfig>,
    shutdown: CancellationToken,
) {
    info!(msg = "sessions manager started");

    let sessions: Arc<Mutex<BTreeMap<String, OpcUaSession>>> = Default::default();

    // Create a stream of ticks for sessions management.
    let mut ticker = interval(MANAGER_TICK_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let stream = IntervalStream::new(ticker).take_until(shutdown.cancelled());
    let mut pinned_stream = pin!(stream);

    while pinned_stream.next().await.is_some() {
        // Drop dead sessions.
        let mut sessions_lock = sessions.lock_arc();
        sessions_lock.retain(|server_id, session| {
            let finished = session.is_finished();
            if finished {
                info!(msg = "dropping dead session", server_id);
            }
            !finished
        });

        // Clone the keys to allow releasing the lock quickly.
        let running_sessions = sessions_lock.keys().cloned().collect::<Vec<_>>();
        drop(sessions_lock);

        // Spawn sessions that are not running.
        for (id, config) in servers
            .iter()
            .filter(|(id, _)| !running_sessions.contains(*id))
        {
            let client = Arc::clone(&client);
            let server_id = id.clone();
            let srv_config = config.clone();
            let registry = Arc::clone(&sessions);
            tokio::spawn(async move {
                let start_future = start_session(client, server_id.clone(), srv_config, registry);
                if timeout(CONNECT_TIMEOUT, start_future).await.is_err() {
                    error!(error = "timeout spawning session", server_id);
                }
            });
        }
    }

    // Take the sessions registry out of the Mutex.
    let sessions = mem::take(sessions.lock_arc().deref_mut());
    // Stop the sessions.
    for session in sessions.into_values() {
        session.stop().await;
    }

    info!(msg = "sessions manager terminated");
}
