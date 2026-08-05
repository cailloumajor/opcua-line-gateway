use thiserror::Error;
use tracing::{info, instrument};

use super::{BrowsePartSheetError, InitialState, TraceabilityHandler};

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {
    #[error("error browsing the general part sheet object")]
    BrowseGeneralPartSheet(#[source] BrowsePartSheetError),
}

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(crate) struct Initialized {
    pub(super) general_part_sheet_nodes: Vec<u32>,
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

        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet_nodes.len()
        );

        let state = Initialized {
            general_part_sheet_nodes,
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
