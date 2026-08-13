use thiserror::Error;
use tracing::{info, instrument};

use super::{BrowsePartSheetError, InitialState, TraceabilityHandler};

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {
    #[error("error browsing the general part sheet object")]
    BrowseGeneralPartSheet(#[source] BrowsePartSheetError),
    #[error("part identifier node not found in general part sheet")]
    NoPartIdNode,
}

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(crate) struct Initialized {
    /// Numeric identifiers of the discovered general part sheet nodes.
    pub(super) general_part_sheet_nodes: Vec<u32>,
    /// Index of the part identifier in the general part sheet nodes collection.
    pub(super) part_id_index: usize,
}

impl TraceabilityHandler<InitialState> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
    pub(crate) async fn initialize(
        self,
    ) -> Result<TraceabilityHandler<Initialized>, TraceabilityInitializeError> {
        let general_part_sheet_nodes = self
            .browse_part_sheet(self.config.general_part_sheet_node_id)
            .await
            .map_err(TraceabilityInitializeError::BrowseGeneralPartSheet)?;
        let part_id_index = general_part_sheet_nodes
            .iter()
            .position(|id| *id == self.config.part_id_node_id)
            .ok_or(TraceabilityInitializeError::NoPartIdNode)?;

        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet_nodes.len()
        );

        let state = Initialized {
            general_part_sheet_nodes,
            part_id_index,
        };

        Ok(TraceabilityHandler {
            server_id: self.server_id,
            config: self.config,
            session: self.session,
            cache: self.cache,
            state,
        })
    }
}
