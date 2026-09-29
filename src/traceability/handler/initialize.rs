use std::sync::Arc;

use opcua::types::NodeId;
use thiserror::Error;
use tracing::{info, instrument};

use super::{BrowsePartSheetError, InitialState, TraceabilityHandler};

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {
    #[error("error getting common traceability namespace index")]
    GetCommonNamespaceIndex(#[source] opcua::types::Error),
    #[error("error browsing the general part sheet object")]
    BrowseGeneralPartSheet(#[source] BrowsePartSheetError),
    #[error("part identifier node not found in general part sheet")]
    NoPartIdNode,
}

/// The traceability handler state after initialization.
pub(crate) struct TraceabilityContext {
    /// Protocol context.
    pub(super) protocol: ProtocolContext,
    /// General part sheet context.
    pub(super) general_part_sheet: GeneralPartSheetContext,
}

/// Traceability protocol context.
pub(super) struct ProtocolContext {
    /// [`NodeId`] of the request variable.
    pub(super) request: NodeId,
    /// [`NodeId`] of the response variable.
    pub(super) response: NodeId,
    /// [`NodeId`] of the heartbeat variable.
    pub(super) heartbeat: NodeId,
}

/// Traceability context related to general part sheet.
pub(super) struct GeneralPartSheetContext {
    /// Discovered nodes, couples of NodeIds and browse name.
    pub(super) nodes: Vec<(NodeId, Arc<str>)>,
    /// Index of the part identifier in the nodes collection.
    pub(super) part_id_index: usize,
    /// [`NodeId`] of the part identifier.
    pub(super) part_id: NodeId,
    /// [`NodeId`] of the raw part reference.
    pub(super) raw_part_ref: NodeId,
    /// [`NodeId`] of the raw part material batch.
    pub(super) raw_batch: NodeId,
}

impl TraceabilityHandler<InitialState> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
    pub(crate) async fn initialize(
        self,
    ) -> Result<TraceabilityHandler<TraceabilityContext>, TraceabilityInitializeError> {
        // Get the namespace index for traceability common part.
        let common_ns_index = self
            .session
            .get_namespace_index(&self.common_opcua_config.namespace_url)
            .await
            .map_err(TraceabilityInitializeError::GetCommonNamespaceIndex)?;

        // Browse the general part sheet object.
        let general_part_sheet_node_id = NodeId::new(
            common_ns_index,
            self.common_opcua_config.general_part_sheet_nid,
        );
        let general_part_sheet_nodes = self
            .browse_part_sheet(general_part_sheet_node_id)
            .await
            .map_err(TraceabilityInitializeError::BrowseGeneralPartSheet)?;

        let part_id_node = NodeId::new(common_ns_index, self.common_opcua_config.part_id_nid);

        // Get the index of the part identifier in general part sheet nodes.
        let part_id_index = general_part_sheet_nodes
            .iter()
            .position(|(id, _)| *id == part_id_node)
            .ok_or(TraceabilityInitializeError::NoPartIdNode)?;

        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet_nodes.len()
        );

        let state = TraceabilityContext {
            protocol: ProtocolContext {
                request: NodeId::new(common_ns_index, self.common_opcua_config.request_nid),
                response: NodeId::new(common_ns_index, self.common_opcua_config.response_nid),
                heartbeat: NodeId::new(common_ns_index, self.common_opcua_config.heartbeat_nid),
            },
            general_part_sheet: GeneralPartSheetContext {
                nodes: general_part_sheet_nodes,
                part_id_index,
                part_id: part_id_node,
                raw_part_ref: NodeId::new(
                    common_ns_index,
                    self.common_opcua_config.raw_part_ref_nid,
                ),
                raw_batch: NodeId::new(common_ns_index, self.common_opcua_config.raw_batch_nid),
            },
        };

        Ok(TraceabilityHandler {
            server_id: self.server_id,
            common_opcua_config: self.common_opcua_config,
            machine_config: self.machine_config,
            session: self.session,
            cache: self.cache,
            state,
        })
    }
}
