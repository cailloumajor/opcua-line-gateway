use std::sync::Arc;

use opcua::types::{ReadValueId, TimestampsToReturn, WriteValue};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, SessionExt, TryFromOpcUaValueError, WriteError};
use crate::traceability::cache::GetGeneralPartSheetError;

use super::{TraceabilityContext, TraceabilityHandler};

/// Errors that can occur during handling the request to read the general part sheet.
#[derive(Debug, Error)]
pub(super) enum HandleReadError {
    #[error("error reading the part ID")]
    ReadPartId(#[source] opcua::types::Error),
    #[error("invalid part ID value, cause: {0}")]
    PartIdValue(#[source] TryFromOpcUaValueError),
    #[error("error getting general part sheet from cache")]
    CacheGet(#[source] GetGeneralPartSheetError),
    #[error("blocking task to retrieve general part sheet failed: {0}")]
    CacheGetTask(JoinError),
    #[error("general part sheet not found for id {0}")]
    CacheMissing(String),
    #[error("error writing the general part sheet to the OPC-UA server")]
    WritePartSheet(#[source] WriteError),
}

impl TraceabilityHandler<TraceabilityContext> {
    /// Run the request from the OPC-UA server to read the part sheet, i.e.
    /// read general part data from the cache and write it to the server.
    #[instrument(err, skip_all)]
    pub(super) async fn handle_read(&self) -> Result<(), HandleReadError> {
        // Get the part ID from the OPC-UA server.
        let read_value_id = ReadValueId::new_value(self.state.general_part_sheet.part_id.clone());
        let values = self
            .session
            .read(&[read_value_id], TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(HandleReadError::ReadPartId)?;
        let [part_id_value] = values
            .try_into()
            .expect("read values vector should have the expected size");
        let part_id: AsciiDigitsOrUpper<23> = part_id_value
            .try_ua_value_as()
            .map_err(HandleReadError::PartIdValue)?;

        // Get the general part sheet from the cache, using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let sent_context = self.session.context();
        let task = tokio::task::spawn_blocking(move || {
            sent_cache.get_general_part_sheet(part_id.as_str(), &sent_context.read_arc().context())
        });
        let part_sheet_from_cache = task
            .await
            .map_err(HandleReadError::CacheGetTask)?
            .map_err(HandleReadError::CacheGet)?;
        let part_sheet = part_sheet_from_cache
            .ok_or_else(|| HandleReadError::CacheMissing(part_id.to_string()))?;

        // Write the general part sheet to the server.
        let write_values = part_sheet
            .into_iter()
            .map(|(id, val)| WriteValue::value_attr(id, val))
            .collect::<Vec<_>>();
        self.session
            .write_checked(&write_values)
            .await
            .map_err(HandleReadError::WritePartSheet)?;

        info!(msg = "general part sheet read", %part_id);

        Ok(())
    }
}
