use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt;
use opcua::client::browser::{Browser, NoneBrowserPolicy};
use opcua::client::{DefaultRetryPolicy, ExponentialBackoff, Session};
use opcua::types::{
    BrowseDescription, BrowseDirection, BrowseResultMaskFlags, NodeClassMask, NodeId, ReadValueId,
    ReferenceTypeId, StatusCode, TimestampsToReturn,
};
use opcua_line_gateway_config::{TraceabilityCommonOpcUaConfig, TraceabilityMachineConfig};
use thiserror::Error;
use tracing::{instrument, warn};

use crate::opcua::{DataValueExt, SerializeVariant, TryFromOpcUaValueError};

use super::cache::TraceabilityCache;

use initialize::TraceabilityContext;
pub(crate) use initialize::TraceabilityInitializeError;
pub(crate) use install::TraceabilityInstallError;

mod create_part_id;
mod handle_request;
mod initialize;
mod install;
mod read_part_sheet;
mod save_part_sheets;

/// Error that can occur during browsing a part sheet object.
#[derive(Debug, Error)]
pub(crate) enum BrowsePartSheetError {
    #[error("error browsing general part sheet OPC-UA nodes")]
    BrowseGeneralPartSheet(#[source] opcua::types::Error),
    #[error("bad BrowseResult status code: {0}")]
    BrowseResultStatus(StatusCode),
    #[error("browse name for node identifier {0} is null")]
    NullBrowseName(NodeId),
    #[error("error with OPC-UA read request")]
    ReadRequest(#[source] opcua::types::Error),
    #[error("invalid number of read variables (discovered {0}, read {1})")]
    ValuesCount(usize, usize),
    #[error("invalid discovered data value for node {1}, cause: {0}")]
    InvalidDataValue(TryFromOpcUaValueError, String),
    #[error("unsupported Variant for serialization for node {1}, cause: {0}")]
    NotSerializable(serde_json::Error, String),
}

/// The initial state of the traceability handler.
pub(crate) struct InitialState;

/// Manages traceability for an OPC-UA session.
pub(crate) struct TraceabilityHandler<S> {
    /// The ID of the machine this handler works with.
    server_id: Arc<str>,
    /// The OPC-UA traceability configuration, common to all machines.
    common_opcua_config: TraceabilityCommonOpcUaConfig,
    /// The traceability configuration for this machine.
    machine_config: TraceabilityMachineConfig,
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
        server_id: Arc<str>,
        common_opcua_config: TraceabilityCommonOpcUaConfig,
        machine_config: TraceabilityMachineConfig,
        session: Arc<Session>,
        cache: Arc<TraceabilityCache>,
    ) -> Self {
        Self {
            server_id,
            common_opcua_config,
            machine_config,
            session,
            cache,
            state: InitialState,
        }
    }
}

impl TraceabilityHandler<InitialState> {
    /// Browse a part sheet (i.e. an OPC-UA object), provided its node identifier.
    /// Return a collection of numeric node identifiers and browse names couples,
    /// which are those of the object's properties of variable type.
    #[instrument(err, skip(self))]
    async fn browse_part_sheet(
        &self,
        root_node_id: NodeId,
    ) -> Result<Vec<(NodeId, Arc<str>)>, BrowsePartSheetError> {
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
            node_id: root_node_id,
            // Browse forward.
            browse_direction: BrowseDirection::Forward,
            // Only follow `HasProperty` references.
            reference_type_id: ReferenceTypeId::HasProperty.into(),
            // Do not include subtypes of reference type.
            include_subtypes: false,
            // Return only nodes of `Variable` class.
            node_class_mask: NodeClassMask::VARIABLE.bits(),
            // Enable `BrowseName` field in the returned `ReferenceDescription`.
            result_mask: BrowseResultMaskFlags::BrowseName.bits(),
        };

        let mut nodes: Vec<(NodeId, Arc<str>)> = Vec::new();

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
                let node_id = ref_description.node_id.node_id.clone();
                if !node_id.is_numeric() {
                    warn!(msg = "non-numeric NodeId found", %node_id);
                }
                let Some(browse_name) = ref_description.browse_name.name.value() else {
                    return Err(BrowsePartSheetError::NullBrowseName(node_id));
                };

                nodes.push((node_id, browse_name.as_str().into()));
            }
        }

        // Read the nodes values and ensure we can serialize them.
        let read_value_ids = nodes
            .iter()
            .map(|(id, _)| ReadValueId::new_value(id.clone()))
            .collect::<Vec<_>>();
        let values = self
            .session
            .read(&read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(BrowsePartSheetError::ReadRequest)?;
        let expected_len = nodes.len();
        let got_len = values.len();
        if got_len != expected_len {
            return Err(BrowsePartSheetError::ValuesCount(expected_len, got_len));
        }
        for (value, (_, name)) in values.into_iter().zip(&nodes) {
            let variant = value
                .try_into_variant()
                .map_err(|err| BrowsePartSheetError::InvalidDataValue(err, name.to_string()))?;
            serde_json::to_value(SerializeVariant(&variant))
                .map_err(|err| BrowsePartSheetError::NotSerializable(err, name.to_string()))?;
        }

        Ok(nodes)
    }
}
