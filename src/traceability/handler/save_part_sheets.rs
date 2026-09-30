use std::sync::Arc;

use opcua::types::{ReadValueId, TimestampsToReturn};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError, TryFromVariant};
use crate::traceability::cache::{CheckEnqueuingError, SavePartSheetsError};
use crate::traceability::part_id::{PartIdentifierError, validate_part_identifier};

use super::{TraceabilityContext, TraceabilityHandler};

/// Errors that can occur during handling the request for saving part sheets.
#[derive(Debug, Error)]
pub(super) enum HandleSaveError {
    #[error("error reading part sheets nodes")]
    ReadPartSheets(#[source] opcua::types::Error),
    #[error("invalid number of variables in part sheets (discovered {0}, read {1})")]
    PartSheetsLength(usize, usize),
    #[error("invalid part sheet value for node {1}, cause: {0}")]
    PartSheetValue(TryFromOpcUaValueError, String),
    #[error("invalid part identifier value, cause: {0}")]
    PartIdValue(TryFromOpcUaValueError),
    #[error("invalid part identifier: {1}")]
    InvalidPartId(#[source] PartIdentifierError, String),
    #[error("error inserting general part sheet in the cache")]
    CacheSave(#[source] SavePartSheetsError),
    #[error("blocking task to cache general part sheet failed: {0}")]
    CacheSaveTask(JoinError),
    #[error("enqueuing part sheets is forbidden")]
    Enqueuing(#[source] CheckEnqueuingError),
}

impl TraceabilityHandler<TraceabilityContext> {
    /// Run the request from the OPC-UA server to save the part sheets, i.e.:
    ///
    /// * read the part sheets from the server,
    /// * write the general part sheet to the cache,
    /// * write all the part sheets to the database.
    #[instrument(err, skip_all)]
    pub(super) async fn handle_save(&self) -> Result<(), HandleSaveError> {
        // Check we are allowed to enqueue part sheets.
        self.cache
            .check_enqueuing_allowed()
            .map_err(HandleSaveError::Enqueuing)?;

        // Build the list of elements (`ReadValueId`s) we want to read from the server,
        // i.e. general part sheet nodes and operation par sheet nodes.
        // The order here is important (general part sheet first), because we take the
        // part identifier value by index in the general part sheet, which must be the
        // same as the index in the "nodes to read" list.
        let read_value_ids = self
            .state
            .general_part_sheet
            .nodes
            .iter()
            .chain(&self.state.operation_part_sheet.nodes)
            .map(|(id, _)| ReadValueId::new_value(id.clone()))
            .collect::<Vec<_>>();

        // Read nodes values from the server.
        let read_values = self
            .session
            .read(&read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(HandleSaveError::ReadPartSheets)?;

        // Ensure we have as many read nodes as requested.
        let expected_len = read_value_ids.len();
        let got_len = read_values.len();
        if got_len != expected_len {
            return Err(HandleSaveError::PartSheetsLength(expected_len, got_len));
        }

        // Build the general part sheet and the operation part sheet, contiguous
        // in the same collection.
        let mut part_sheets = self
            .state
            .general_part_sheet
            .nodes
            .iter()
            .chain(&self.state.operation_part_sheet.nodes)
            .zip(read_values)
            .map(|((id, name), val)| {
                val.try_into_variant()
                    .map(|variant| (id.clone(), Arc::clone(name), variant))
                    .map_err(|err| HandleSaveError::PartSheetValue(err, name.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Get the part identifier.
        let part_id_variant = part_sheets
            .get(self.state.general_part_sheet.part_id_index)
            .map(|(_, _, v)| v)
            .expect("an element should exist at the part identifier index position");
        let part_id = AsciiDigitsOrUpper::<23>::try_from_variant(part_id_variant.clone())
            .map_err(HandleSaveError::PartIdValue)?;
        validate_part_identifier(part_id)
            .map_err(|err| HandleSaveError::InvalidPartId(err, part_id.to_string()))?;

        // Get the operation part sheet out of the collection.
        let operation_part_sheet = part_sheets.split_off(self.state.general_part_sheet.nodes.len());
        // Remaining elements after splitting off the operation part sheet are
        // general part sheet elements.
        let general_part_sheet = part_sheets;

        // Insert the part sheets in the cache, using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let sent_server_id = Arc::clone(&self.server_id);
        let sent_context = self.session.context();
        let task = tokio::task::spawn_blocking(move || {
            sent_cache.save_part_sheets(
                sent_server_id,
                part_id,
                &general_part_sheet,
                &operation_part_sheet,
                &sent_context.read_arc().context(),
            )
        });

        task.await
            .map_err(HandleSaveError::CacheSaveTask)?
            .map_err(HandleSaveError::CacheSave)?;

        info!(msg = "part sheets saved", %part_id);

        Ok(())
    }
}
