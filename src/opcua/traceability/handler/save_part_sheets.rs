use std::iter;

use opcua::types::Variant;
use thiserror::Error;
use tracing::{info, instrument, warn};

use crate::opcua::data_value::{DataValueExt, TryFromDataValueError};

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
        // Read general part sheet values from the server.
        let general_part_sheet_values = self
            .read_values(&self.state.general_part_sheet_nodes)
            .await
            .map_err(SavePartSheetsError::ReadGeneralPartSheet)?;

        // Convert values to variants and find the part identifier.
        let mut general_part_sheet = Vec::with_capacity(general_part_sheet_values.len());
        let mut maybe_part_id = None;
        for (id, val) in iter::zip(
            &self.state.general_part_sheet_nodes,
            &general_part_sheet_values,
        ) {
            let variant: &Variant = val
                .try_as()
                .map_err(|err| SavePartSheetsError::GeneralPartSheetValue(err, *id))?;
            general_part_sheet.push((id, variant));

            if maybe_part_id.is_none() && *id == self.config.part_id_node_id {
                let part_id: &str = val.try_as().map_err(SavePartSheetsError::PartIdValue)?;
                maybe_part_id = Some(part_id)
            }
        }

        let part_id = maybe_part_id.ok_or(SavePartSheetsError::NoPartIdNode)?;

        // Encode and insert the general part sheet in the cache, using a blocking task.
        tokio::task::block_in_place(move || {
            self.cache
                .insert_general_part_sheet(
                    part_id,
                    &general_part_sheet,
                    &self.session.encoding_context().read().context(),
                )
                .map_err(SavePartSheetsError::CacheInsert)
        })?;

        // TODO: write part sheets to the database.
        warn!(msg = "part sheets insertion to database is not yet implemented");

        info!(msg = "part sheets saved", part_id);

        Ok(())
    }
}
