use opcua::types::Variant;
use thiserror::Error;
use tracing::{info, instrument, warn};

use crate::opcua::data_value::{DataValueExt, TryFromDataValueError};
use crate::opcua::traceability::part_sheet::CachedPartSheet;

use super::{Initialized, ReadError, TraceabilityHandler};

/// Errors that can occur during handling the request for saving part sheets.
#[derive(Debug, Error)]
pub(super) enum SavePartSheetsError {
    #[error("error reading general part sheet nodes")]
    ReadGeneralPartSheet(#[source] ReadError),
    #[error("invalid general part sheet member value (id={1}), cause: {0}")]
    GeneralPartSheetValue(TryFromDataValueError, u32),
    #[error("part identifier node not found in general part sheet")]
    NoPartIdNode,
    #[error("invalid part identifier value, cause: {0}")]
    PartIdValue(TryFromDataValueError),
    #[error("error inserting general part sheet in the cache")]
    CacheInsert(#[source] redb::Error),
}

impl TraceabilityHandler<Initialized> {
    /// Run the request from the OPC-UA server to save the part sheets, i.e.:
    ///
    /// * read the part sheets from the server,
    /// * write the general part sheet to the cache,
    /// * write all the part sheets to the database.
    #[instrument(err, skip_all)]
    pub(super) async fn save_part_sheets(&self) -> Result<(), SavePartSheetsError> {
        // Read general part sheet values form the server.
        let general_part_sheet_values = self
            .read_values(&self.state.general_part_sheet_nodes)
            .await
            .map_err(SavePartSheetsError::ReadGeneralPartSheet)?;
        let general_part_sheet: Vec<&Variant> = general_part_sheet_values
            .iter()
            .zip(&self.state.general_part_sheet_nodes)
            .map(|(val, id)| {
                val.try_as()
                    .map_err(|err| SavePartSheetsError::GeneralPartSheetValue(err, *id))
            })
            .collect::<Result<_, _>>()?;

        // Find the part identifier in the general part sheet.
        let part_id_value = self
            .state
            .general_part_sheet_nodes
            .iter()
            .zip(&general_part_sheet_values)
            .find_map(|(id, value)| self.config.part_id_node_id.eq(id).then_some(value))
            .ok_or(SavePartSheetsError::NoPartIdNode)?;
        let part_id: &str = part_id_value
            .try_as()
            .map_err(SavePartSheetsError::PartIdValue)?;

        // Encode and insert the general part sheet in the cache, using a blocking task.
        tokio::task::block_in_place(move || {
            let pairs = self
                .state
                .general_part_sheet_nodes
                .iter()
                .copied()
                .zip(general_part_sheet);
            let encoded =
                CachedPartSheet::encode(pairs, &self.session.encoding_context().read().context());
            self.cache
                .insert_general_part_sheet(part_id, &encoded)
                .map_err(SavePartSheetsError::CacheInsert)
        })?;

        // TODO: write part sheets to the database.
        warn!(msg = "part sheets insertion to database is not yet implemented");

        info!(msg = "part sheets saved");

        Ok(())
    }
}
