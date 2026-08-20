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
pub(crate) struct TraceabilityContext {
    /// General part sheet context.
    pub(super) general_part_sheet: GeneralPartSheetContext,
}

/// Traceability context related to general part sheet.
#[derive(Clone)]
pub(super) struct GeneralPartSheetContext {
    /// Discovered nodes, couples of numeric identifier and browse name.
    pub(super) nodes: Vec<(u32, String)>,
    /// Index of the part identifier in the nodes collection.
    pub(super) part_id_index: usize,
}

impl TraceabilityHandler<InitialState> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
    pub(crate) async fn initialize(
        self,
    ) -> Result<TraceabilityHandler<TraceabilityContext>, TraceabilityInitializeError> {
        let general_part_sheet_nodes = self
            .browse_part_sheet(self.config.nodes.general_part_sheet)
            .await
            .map_err(TraceabilityInitializeError::BrowseGeneralPartSheet)?;
        let part_id_index = general_part_sheet_nodes
            .iter()
            .position(|(id, _)| *id == self.config.nodes.part_id)
            .ok_or(TraceabilityInitializeError::NoPartIdNode)?;

        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet_nodes.len()
        );

        let state = TraceabilityContext {
            general_part_sheet: GeneralPartSheetContext {
                nodes: general_part_sheet_nodes,
                part_id_index,
            },
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
