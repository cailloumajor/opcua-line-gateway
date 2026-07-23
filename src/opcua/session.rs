use std::collections::BTreeMap;
use std::sync::Arc;

use opcua::client::transport::TcpConnector;
use opcua::client::{Client, Session, SessionEventLoop};
use opcua::types::StatusCode;
use opcua_line_gateway_config::OpcUaServerConfig;
use parking_lot::Mutex;
use redb::Database;
use thiserror::Error;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use tracing::{Instrument, error, info, info_span, instrument};

use crate::opcua::traceability::{
    TraceabilityHandler, TraceabilityInitializeError, TraceabilityInstallError,
};

/// Errors that can occur during session creation.
#[derive(Debug, Error)]
pub(super) enum CreateSessionError {
    #[error("error getting server endpoints")]
    GetServerEndpoints(#[source] opcua::types::Error),
    #[error("error adding endpoint to session builder")]
    AddEndpointToSessionBuilder(#[source] opcua::types::Error),
    #[error("error building the session")]
    SessionBuild(#[source] opcua::types::Error),
    #[error("error connecting the session")]
    SessionConnect,
    #[error("error initializing traceability handler")]
    TraceabilityInitialize(#[from] TraceabilityInitializeError),
    #[error("error installing traceability handler")]
    TraceabilityInstall(#[from] TraceabilityInstallError),
}

/// Represents an active OPC-UA session.
pub(super) struct OpcUaSession {
    /// The ID of the server this session connects to.
    server_id: String,
    /// The wrapped OPC-UA client session.
    session: Arc<Session>,
    /// The handle to the session runtime task.
    event_loop_handle: JoinHandle<StatusCode>,
    /// The token to ask traceability handler to shutdown gracefully.
    traceability_cancel: CancellationToken,
    /// The collection of traceability tasks.
    traceability_tasks: JoinSet<()>,
}

impl OpcUaSession {
    /// Ask the session to stop and wait for the operation to complete.
    ///
    /// This function takes ownership of the [`OpcUaSession`].
    #[instrument(skip_all, fields(server_id = self.server_id))]
    pub(super) async fn stop(mut self) {
        info!(msg = "stopping session");

        // Stop traceability tasks.
        self.traceability_cancel.cancel();
        while let Some(result) = self.traceability_tasks.join_next().await {
            if let Err(err) = result {
                error!(error = "error joining traceability tasks: {}", %err);
            }
        }

        // Stop the session.
        if let Err(err) = self.session.disconnect().await {
            error!(error = "error disconnecting the session: {}", %err);
        }
        if let Err(err) = self.event_loop_handle.await {
            error!(error = "error joining session event loop task: {}", %err);
        }
    }

    /// Check whether the session event loop task is finished.
    pub(super) fn is_finished(&self) -> bool {
        self.event_loop_handle.is_finished()
    }
}

/// Create and start an OPC-UA session, spawning its runtime event loop.
///
/// Upon success, the [`OpcUaSession`] object will be stored in the provided registry.
#[instrument(err, skip(client, server_config, registry))]
pub(super) async fn start_session(
    client: Arc<Client>,
    server_id: String,
    server_config: OpcUaServerConfig,
    traceability_cache_db: Arc<Database>,
    registry: Arc<Mutex<BTreeMap<String, OpcUaSession>>>,
) -> Result<(), CreateSessionError> {
    info!(msg = "creating OPC-UA session");

    let (session, event_loop) = connect_to_matching_endpoint(&client, &server_config).await?;

    // Disable session reconnection, we handle it ourselves.
    session.disable_reconnects();

    // Start polling the event loop to bring the session alive.
    let event_loop_handle = tokio::spawn(
        event_loop
            .run()
            .instrument(info_span!(parent: None, "session_event_loop", server_id)),
    );

    // Allow the event loop handling task to be aborted if anything goes wrong before
    // the end of this scope.
    let loop_abort_handle = AbortOnDropHandle::new(event_loop_handle);

    if !session.wait_for_connection().await {
        return Err(CreateSessionError::SessionConnect);
    }

    let traceability_cancel = CancellationToken::new();
    let traceability_handler = TraceabilityHandler::new(
        server_id.clone(),
        server_config.traceability,
        Arc::clone(&session),
        Arc::clone(&traceability_cache_db),
    );
    let traceability_tasks = traceability_handler
        .initialize()
        .await?
        .install(traceability_cancel.clone())
        .await?;

    // Get back the event loop handle and disable the abort-on-drop effect.
    let event_loop_handle = loop_abort_handle.detach();

    let opcua_session = OpcUaSession {
        server_id: server_id.clone(),
        session,
        event_loop_handle,
        traceability_cancel,
        traceability_tasks,
    };

    registry.lock_arc().insert(server_id, opcua_session);

    Ok(())
}

/// Replacement implementation of [`Client::connect_to_matching_endpoint()`].
///
/// This is a workaround which does not unnecessarily take an exclusive reference to the client.
async fn connect_to_matching_endpoint(
    client: &Client,
    server_config: &OpcUaServerConfig,
) -> Result<(Arc<Session>, SessionEventLoop<TcpConnector>), CreateSessionError> {
    let endpoint_description = server_config.endpoint_description();
    let identity_token = server_config.identity_token();

    let endpoint_url = endpoint_description.endpoint_url.as_ref();
    let endpoints = client
        .get_server_endpoints_from_url(endpoint_url)
        .await
        .map_err(CreateSessionError::GetServerEndpoints)?;
    let session_builder = client
        .session_builder()
        .with_endpoints(endpoints)
        .user_identity_token(identity_token)
        .connect_to_matching_endpoint(endpoint_description)
        .map_err(CreateSessionError::AddEndpointToSessionBuilder)?;
    session_builder
        .build(Arc::clone(client.certificate_store()))
        .map_err(CreateSessionError::SessionBuild)
}
