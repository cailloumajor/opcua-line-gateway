use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt;
use opcua::client::browser::{Browser, NoneBrowserPolicy};
use opcua::client::{DefaultRetryPolicy, ExponentialBackoff, Session};
use opcua::types::{
    BrowseDescription, BrowseDirection, BrowseResultMaskFlags, DataValue, Identifier,
    NodeClassMask, NodeId, ReadValueId, ReferenceTypeId, StatusCode, TimestampsToReturn, Variant,
    WriteValue,
};
use opcua_line_gateway_config::TraceabilityConfig;
use thiserror::Error;
use tracing::instrument;

use super::cache::TraceabilityCache;

use initialize::Initialized;
pub(crate) use initialize::TraceabilityInitializeError;
pub(crate) use install::TraceabilityInstallError;

mod create_part_id;
mod handle_request;
mod initialize;
mod install;
mod read_part_sheet;
mod save_part_sheets;

/// Errors that can occur during reading from the server.
#[derive(Debug, Error)]
enum ReadError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("read request error")]
    ReadRequest(#[source] opcua::types::Error),
}

/// Errors that can be encountered during writing to the server.
#[derive(Debug, Error)]
enum WriteError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("write request error")]
    WriteRequest(#[source] opcua::types::Error),
    #[error("write operation error: {0}")]
    WriteStatus(StatusCode),
}

/// Error that can occur during browsing a part sheet object.
#[derive(Debug, Error)]
pub(crate) enum BrowsePartSheetError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("error browsing general part sheet OPC-UA nodes")]
    BrowseGeneralPartSheet(#[source] opcua::types::Error),
    #[error("bad BrowseResult status code: {0}")]
    BrowseResultStatus(StatusCode),
    #[error("invalid node identifier, expected numeric, got {0}")]
    NonNumericId(Identifier),
}

/// The initial state of the traceability handler.
pub(crate) struct InitialState;

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
    cache: Arc<TraceabilityCache>,
    /// The state of this handler.
    state: S,
}

impl TraceabilityHandler<InitialState> {
    /// Create a new [`TraceabilityHandler`].
    pub(crate) fn new(
        server_id: String,
        config: TraceabilityConfig,
        session: Arc<Session>,
        cache: Arc<TraceabilityCache>,
    ) -> Self {
        Self {
            server_id,
            config,
            session,
            cache,
            state: InitialState,
        }
    }
}

impl TraceabilityHandler<InitialState> {
    /// Browse a part sheet (i.e. an OPC-UA object), provided its node identifier.
    /// Return a collection of node identifiers, which are those of the object's
    /// properties of variable type.
    #[instrument(err, skip(self))]
    async fn browse_part_sheet(&self, root_node_id: u32) -> Result<Vec<u32>, BrowsePartSheetError> {
        // Get the traceability namespace index.
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(BrowsePartSheetError::GetNamespaceIndex)?;

        // Prepare the browser configuration.
        let retry_policy = DefaultRetryPolicy::new(ExponentialBackoff::new(
            Duration::from_secs(5),     // max sleep
            Some(3),                    // max retries
            Duration::from_millis(500), // initial sleep
        ));
        let cloned_session = Arc::clone(&self.session);
        let browser = Browser::new(&cloned_session, NoneBrowserPolicy, retry_policy);
        let initial = BrowseDescription {
            // Start browsing at the part sheet object.
            node_id: NodeId::new(ns_index, root_node_id),
            // Browse forward.
            browse_direction: BrowseDirection::Forward,
            // Only follow `HasProperty` references.
            reference_type_id: ReferenceTypeId::HasProperty.into(),
            // Do not include subtypes of reference type.
            include_subtypes: false,
            // Return only nodes of `Variable` class.
            node_class_mask: NodeClassMask::VARIABLE.bits(),
            // Disable all fields in the returned `ReferenceDescription`.
            // Browse configuration above is restrictive enough to not have the need for them.
            result_mask: BrowseResultMaskFlags::empty().bits(),
        };

        let mut part_sheet_nodes = Vec::new();

        // Browse the part sheet object to build the node identifiers list.
        let mut pinned_stream = pin!(browser.run(vec![initial]));
        while let Some(item) = pinned_stream
            .try_next()
            .await
            .map_err(BrowsePartSheetError::BrowseGeneralPartSheet)?
        {
            let status = item.status();
            if !status.is_good() {
                return Err(BrowsePartSheetError::BrowseResultStatus(status));
            }
            for ref_description in item.references() {
                let identifier = &ref_description.node_id.node_id.identifier;
                let Identifier::Numeric(numeric_id) = identifier else {
                    return Err(BrowsePartSheetError::NonNumericId(identifier.clone()));
                };
                part_sheet_nodes.push(*numeric_id);
            }
        }

        Ok(part_sheet_nodes)
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
