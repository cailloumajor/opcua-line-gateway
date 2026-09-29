use opcua::client::Session;
use opcua::types::{StatusCode, WriteValue};
use thiserror::Error;
use tracing::instrument;

/// Errors that can be encountered during writing to the server.
#[derive(Debug, Error)]
pub(crate) enum WriteError {
    #[error("write request error")]
    WriteRequest(#[source] opcua::types::Error),
    #[error("write operation error: {0}")]
    WriteStatus(StatusCode),
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
        if let Some(status) = results.into_iter().find(|s| !s.is_good()) {
            return Err(WriteError::WriteStatus(status));
        }

        Ok(())
    }
}
