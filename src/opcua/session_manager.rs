use std::collections::BTreeMap;
use std::mem;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Duration;

use opcua::client::Client;
use opcua_line_gateway_config::OpcUaServerConfig;
use parking_lot::Mutex;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};

use crate::opcua::session::{OpcUaSession, spawn_session};

/// The interval at which missing sessions should be started.
const SESSIONS_RESTART_PERIOD: Duration = Duration::from_secs(5);

/// Manages OPC-UA sessions.
pub(crate) struct SessionManager {
    /// Configured OPC-UA servers.
    servers: BTreeMap<String, OpcUaServerConfig>,
    /// Shareable OPC-UA client.
    opcua_client: Arc<Client>,
    /// Sessions registry.
    sessions: Arc<Mutex<BTreeMap<String, OpcUaSession>>>,
}

impl SessionManager {
    /// Create a new [`SessionManager`].
    pub(crate) fn new(
        servers: BTreeMap<String, OpcUaServerConfig>,
        opcua_client: Arc<Client>,
    ) -> Self {
        Self {
            servers,
            opcua_client,
            sessions: Default::default(),
        }
    }

    /// Run the session manager. This is intended to be the main program loop.
    ///
    /// This method consumes the [`SessionManager`].
    #[instrument(name = "session_manager", skip_all)]
    pub(crate) async fn run(self, shutdown_token: CancellationToken) {
        info!(msg = "starting session manager");

        let mut sessions_start_interval = interval(SESSIONS_RESTART_PERIOD);
        sessions_start_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    self.stop().await;
                    break;
                }

                _ = sessions_start_interval.tick() => {
                    self.spawn_missing_sessions().await;
                }
            }
        }

        info!(msg = "session manager terminated");
    }

    /// Spawn sessions in the configuration that are not registered.
    #[instrument(skip_all)]
    async fn spawn_missing_sessions(&self) {
        // Clone the registry keys to keep the lock as shortly as possible.
        let registered_servers = self.sessions.lock_arc().keys().cloned().collect::<Vec<_>>();

        for (server_id, server_config) in self
            .servers
            .iter()
            // We got the list of registered servers from BTreeMap keys iterator,
            // so it is sorted.
            .filter(|(id, _)| registered_servers.binary_search(*id).is_err())
        {
            // Ignore the result here, as it is handled by the instrumentation
            // of the called function.
            let _ =
                spawn_session(&self.opcua_client, server_id, server_config, &self.sessions).await;
        }
    }

    /// Stop the manager by stopping all sessions from the registry.
    async fn stop(self) {
        info!(msg = "stopping session manager");

        let sessions = mem::take(self.sessions.lock_arc().deref_mut());
        for session in sessions.into_values() {
            // Ignore the result here, as it is handled by the instrumentation
            // of the called function.
            let _ = session.stop().await;
        }
    }
}
