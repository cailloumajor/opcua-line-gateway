use std::sync::Arc;

use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument, warn};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError, TryFromVariant};
use crate::traceability::cache::SavePartSheetsError;

use super::{ReadError, TraceabilityContext, TraceabilityHandler};

/// Errors that can occur during handling the request for saving part sheets.
#[derive(Debug, Error)]
pub(super) enum HandleSaveError {
    #[error("error reading general part sheet nodes")]
    ReadGeneralPartSheet(#[source] ReadError),
    #[error("invalid number of variables in general part sheet (discovered {0}, read {1})")]
    GeneralPartSheetLength(usize, usize),
    #[error("invalid general part sheet member value (id={1}), cause: {0}")]
    GeneralPartSheetValue(TryFromOpcUaValueError, u32),
    #[error("invalid part identifier value, cause: {0}")]
    PartIdValue(TryFromOpcUaValueError),
    #[error("error inserting general part sheet in the cache")]
    CacheSave(#[source] SavePartSheetsError),
    #[error("blocking task to cache general part sheet failed: {0}")]
    CacheSaveTask(JoinError),
}

impl TraceabilityHandler<TraceabilityContext> {
    /// Run the request from the OPC-UA server to save the part sheets, i.e.:
    ///
    /// * read the part sheets from the server,
    /// * write the general part sheet to the cache,
    /// * write all the part sheets to the database.
    #[instrument(err, skip_all)]
    pub(super) async fn handle_save(&self) -> Result<(), HandleSaveError> {
        // Read general part sheet values from the server.
        let general_part_sheet_ids = self
            .state
            .general_part_sheet
            .nodes
            .iter()
            .map(|(id, _)| *id);
        let general_part_sheet_values = self
            .read_values(general_part_sheet_ids)
            .await
            .map_err(HandleSaveError::ReadGeneralPartSheet)?;

        let expected_len = self.state.general_part_sheet.nodes.len();
        let got_len = general_part_sheet_values.len();
        if got_len != expected_len {
            return Err(HandleSaveError::GeneralPartSheetLength(
                expected_len,
                got_len,
            ));
        }

        // Build the part sheet.
        let general_part_sheet = self
            .state
            .general_part_sheet
            .nodes
            .iter()
            .zip(general_part_sheet_values)
            .map(|((id, name), val)| {
                val.try_into_variant()
                    .map(|variant| (*id, name.clone(), variant))
                    .map_err(|err| HandleSaveError::GeneralPartSheetValue(err, *id))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Get the part identifier.
        let part_id_variant = general_part_sheet
            .get(self.state.general_part_sheet.part_id_index)
            .map(|(_, _, v)| v)
            .expect("an element should exist at the part identifier index position");
        let part_id = String::try_from_variant(part_id_variant.clone())
            .map_err(HandleSaveError::PartIdValue)?;

        // Insert the general part sheet in the cache, using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let sent_part_id = part_id.to_owned();
        let sent_context = self.session.context();
        let task = tokio::task::spawn_blocking(move || {
            sent_cache.save_part_sheets(
                &sent_part_id,
                &general_part_sheet,
                &sent_context.read_arc().context(),
            )
        });

        task.await
            .map_err(HandleSaveError::CacheSaveTask)?
            .map_err(HandleSaveError::CacheSave)?;

        // TODO: write part sheets to the database.
        warn!(msg = "part sheets insertion to database is not yet implemented");

        info!(msg = "part sheets saved", part_id);

        Ok(())
    }
}
