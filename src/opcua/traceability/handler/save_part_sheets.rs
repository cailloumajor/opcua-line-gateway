use thiserror::Error;
use tracing::{info, instrument, warn};

use crate::opcua::data_value::{DataValueExt, TryFromOpcUaValueError, TryFromVariant};

use super::{Initialized, ReadError, TraceabilityHandler};

/// Errors that can occur during handling the request for saving part sheets.
#[derive(Debug, Error)]
pub(super) enum SavePartSheetsError {
    #[error("error reading general part sheet nodes")]
    ReadGeneralPartSheet(#[source] ReadError),
    #[error("invalid number of variables in general part sheet (discovered {0}, read {1})")]
    GeneralPartSheetLength(usize, usize),
    #[error("invalid general part sheet member value (id={1}), cause: {0}")]
    GeneralPartSheetValue(TryFromOpcUaValueError, u32),
    #[error("invalid part identifier value, cause: {0}")]
    PartIdValue(TryFromOpcUaValueError),
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

        let expected_len = self.state.general_part_sheet_nodes.len();
        let got_len = general_part_sheet_values.len();
        if got_len != expected_len {
            return Err(SavePartSheetsError::GeneralPartSheetLength(
                expected_len,
                got_len,
            ));
        }

        // Convert values to variants.
        let general_part_sheet = self
            .state
            .general_part_sheet_nodes
            .iter()
            .zip(&general_part_sheet_values)
            .map(|(id, val)| {
                val.try_get_variant()
                    .map(|variant| (id, variant))
                    .map_err(|err| SavePartSheetsError::GeneralPartSheetValue(err, *id))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Get the part identifier.
        let (_, part_id_variant) = general_part_sheet[self.state.part_id_index];
        let part_id: &str = TryFromVariant::try_from_variant(part_id_variant)
            .map_err(SavePartSheetsError::PartIdValue)?;

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
