use std::sync::Arc;

use opcua::client::Session;
use opcua::types::{
    DataValue, NodeId, ReadValueId, StatusCode, TimestampsToReturn, Variant, WriteValue,
};
use opcua_line_gateway_config::TraceabilityConfig;
use redb::Database;
use thiserror::Error;
use tracing::instrument;

use super::cache::TraceabilityCache;

pub(crate) use initialize::TraceabilityInitializeError;
pub(crate) use install::TraceabilityInstallError;

mod create_part_id;
mod handle_request;
mod initialize;
mod install;

/// Errors that can occur during reading from the server.
#[derive(Debug, Error)]
pub(super) enum ReadError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("read request error")]
    ReadRequest(#[source] opcua::types::Error),
}

/// Errors that can be encountered during writing to the server.
#[derive(Debug, Error)]
pub(super) enum WriteError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("write request error")]
    WriteRequest(#[source] opcua::types::Error),
    #[error("write operation error: {0}")]
    WriteStatus(StatusCode),
}

/// The initial state of the traceability handler.
pub(crate) struct InitialState;

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(crate) struct Initialized {}

/// Manages traceability for an OPC-UA session.
#[derive(Clone)]
pub(crate) struct TraceabilityHandler<S> {
    /// The ID of the server this handler works with.
    server_id: String,
    /// The configuration for this server.
    config: TraceabilityConfig,
    /// The OPC-UA session.
    session: Arc<Session>,
    /// The traceability cache.
    cache: TraceabilityCache,
    /// The state of this handler.
    state: S,
}

impl TraceabilityHandler<InitialState> {
    /// Create a new [`TraceabilityHandler`].
    pub(crate) fn new(
        server_id: String,
        config: TraceabilityConfig,
        session: Arc<Session>,
        cache_db: Arc<Database>,
    ) -> Self {
        let cache = TraceabilityCache::new(cache_db);

        Self {
            server_id,
            config,
            session,
            cache,
            state: InitialState,
        }
    }
}

impl TraceabilityHandler<Initialized> {
    /// Read the values of nodes with provided identifiers.
    #[instrument(err, skip_all)]
    async fn read_values(&self, ids: &[u32]) -> Result<Vec<DataValue>, ReadError> {
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(ReadError::GetNamespaceIndex)?;
        let nodes_to_read = ids
            .iter()
            .map(|id| {
                let node_id = NodeId::new(ns_index, *id);
                ReadValueId::new_value(node_id)
            })
            .collect::<Vec<_>>();
        self.session
            .read(&nodes_to_read, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(ReadError::ReadRequest)
    }

    /// Write provided values — an iterable of tuples of node identifier ([`u32`])
    /// and [`Variant`] — to the server.
    #[instrument(err, skip_all)]
    async fn write_values<I>(&self, pairs: I) -> Result<(), WriteError>
    where
        I: IntoIterator<Item = (u32, Variant)>,
    {
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(WriteError::GetNamespaceIndex)?;
        let nodes_to_write = pairs
            .into_iter()
            .map(|(id, variant)| WriteValue::value_attr(NodeId::new(ns_index, id), variant))
            .collect::<Vec<_>>();
        let results = self
            .session
            .write(&nodes_to_write)
            .await
            .map_err(WriteError::WriteRequest)?;
        if let Some(status) = results.into_iter().find(|s| !s.is_good()) {
            return Err(WriteError::WriteStatus(status));
        }

        Ok(())
    }
}
