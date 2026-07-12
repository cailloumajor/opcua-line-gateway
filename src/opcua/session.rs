use std::sync::Arc;

use opcua::client::transport::TcpConnector;
use opcua::client::{Client, Session, SessionEventLoop};
use opcua::types::StatusCode;
use opcua_line_gateway_config::OpcUaServerConfig;
use thiserror::Error;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::task::AbortOnDropHandle;
use tracing::{Instrument, info, info_span, instrument};

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
    /// Create an new [`OpcUaSession`] and spawn its runtime event loop.
    #[instrument(name = "spawn_session", err, skip(client, server_config))]
    pub(super) async fn spawn(
        client: Arc<Client>,
        server_id: String,
        server_config: OpcUaServerConfig,
    ) -> Result<Self, CreateSessionError> {
        info!(msg = "creating OPC-UA session");

        let (session, event_loop) = connect_to_matching_endpoint(&client, &server_config).await?;

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

        // TODO: plug traceability here

        // Get back the event loop handle and disable the abort-on-drop effect.
        let event_loop_handle = loop_abort_handle.detach();

        Ok(Self {
            server_id,
            session,
            event_loop_handle,
        })
    }

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
