use opcua::client::Session;
use opcua::types::{NodeId, StatusCode, WriteValue};
use thiserror::Error;
use tracing::instrument;

/// Errors that can be encountered during writing to the server.
#[derive(Debug, Error)]
pub(crate) enum WriteError {
    #[error("write request error")]
    WriteRequest(#[source] opcua::types::Error),
    #[error("write operation error: {0}, node_id: {1}")]
    WriteStatus(StatusCode, NodeId),
}

/// Extension trait for OPC-UA session.
pub(crate) trait SessionExt {
    /// Write values to the server, checking returned status codes.
    async fn write_checked(&self, values: &[WriteValue]) -> Result<(), WriteError>;
}

impl SessionExt for Session {
    #[instrument(err, skip_all)]
    async fn write_checked(&self, nodes_to_write: &[WriteValue]) -> Result<(), WriteError> {
        let results = self
            .write(nodes_to_write)
            .await
            .map_err(WriteError::WriteRequest)?;
        if let Some((index, status)) = results.into_iter().enumerate().find(|(_, s)| !s.is_good()) {
            let node_id = nodes_to_write[index].node_id.clone();
            return Err(WriteError::WriteStatus(status, node_id));
        }

        Ok(())
    }
}
