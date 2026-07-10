use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use opcua::client::{Client, Session};
use opcua::types::StatusCode;
use opcua_line_gateway_config::OpcUaServerConfig;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::task::AbortOnDropHandle;
use tracing::{Instrument, info, info_span, instrument};

/// The maximum time allowed for the session to be connected.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Errors that can occur during session stopping.
#[derive(Debug, Error)]
pub(super) enum SessionStopError {
    #[error("error disconnecting session")]
    Disconnect(#[source] StatusCode),
    #[error("error joining session event loop task")]
    JoinLoopTask(#[source] JoinError),
}

/// Errors that can occur during session creation.
#[derive(Debug, Error)]
pub(super) enum CreateSessionError {
    #[error("error getting server endpoints")]
    GetServerEndpoints(#[source] opcua::types::Error),
    #[error("error adding endpoint to session builder")]
    AddEndpointToSessionBuilder(#[source] opcua::types::Error),
    #[error("error building the session")]
    SessionBuild(#[source] opcua::types::Error),
    #[error("timeout waiting for session connection")]
    SessionConnectTimeout,
    #[error("error connecting the session")]
    SessionConnect,
}

/// Represents an active OPC-UA session.
pub(super) struct OpcUaSession {
    /// The ID of the server this session connects to.
    server_id: String,
    /// The wrapped OPC-UA client session.
    session: Arc<Session>,
    /// The handle to the session runtime task.
    event_loop_handle: JoinHandle<StatusCode>,
}

impl OpcUaSession {
    /// Ask the session to stop and wait for the operation to complete.
    ///
    /// This function takes ownership of the [`OpcUaSession`].
    #[instrument(err, skip(self), fields(server_id = self.server_id))]
    pub(super) async fn stop(self) -> Result<(), SessionStopError> {
        info!(msg = "stopping session");

        self.session
            .disconnect()
            .await
            .map_err(SessionStopError::Disconnect)?;
        self.event_loop_handle
            .await
            .map_err(SessionStopError::JoinLoopTask)?;

        Ok(())
    }
}

/// Create an OPC-UA client session and spawn its runtime event loop.
///
/// Upon success, the created [`OpcUaSession`] will be stored in the provided registry.
#[instrument(err, parent = None, skip(client, server_config, session_registry))]
pub(super) async fn spawn_session(
    client: &Client,
    server_id: &str,
    server_config: &OpcUaServerConfig,
    session_registry: &Arc<Mutex<BTreeMap<String, OpcUaSession>>>,
) -> Result<(), CreateSessionError> {
    info!(msg = "creating OPC-UA session");

    let endpoint_description = server_config.endpoint_description();
    let identity_token = server_config.identity_token();

    // Workaround for `Client::connect_to_matching_endpoint` unnecessarily taking
    // an exclusive reference to the client.
    let endpoints = client
        .get_server_endpoints_from_url(endpoint_description.endpoint_url.as_ref())
        .await
        .map_err(CreateSessionError::GetServerEndpoints)?;
    let session_builder = client
        .session_builder()
        .with_endpoints(endpoints)
        .user_identity_token(identity_token)
        .connect_to_matching_endpoint(endpoint_description)
        .map_err(CreateSessionError::AddEndpointToSessionBuilder)?;
    let (session, event_loop) = session_builder
        .build(Arc::clone(client.certificate_store()))
        .map_err(CreateSessionError::SessionBuild)?;

    // Start polling the event loop to bring the session alive.
    let event_loop_handle = tokio::spawn(
        event_loop
            .run()
            .instrument(info_span!(parent: None, "session_event_loop", server_id)),
    );

    // Allow the event loop handling task to be aborted if anything goes wrong before
    // the end of this scope.
    let loop_abort_handle = AbortOnDropHandle::new(event_loop_handle);

    match tokio::time::timeout(CONNECT_TIMEOUT, session.wait_for_connection()).await {
        Ok(true) => {}
        Ok(false) => return Err(CreateSessionError::SessionConnect),
        Err(_) => return Err(CreateSessionError::SessionConnectTimeout),
    }

    // TODO: plug traceability here

    // Get back the event loop handle and disable the abort-on-drop effect.
    let event_loop_handle = loop_abort_handle.detach();

    let opcua_session = OpcUaSession {
        server_id: server_id.to_owned(),
        session,
        event_loop_handle,
    };

    session_registry
        .lock_arc()
        .insert(server_id.to_owned(), opcua_session);

    Ok(())
}
