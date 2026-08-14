use thiserror::Error;
use tracing::{info, instrument};

use crate::opcua::data_value::{DataValueExt, TryFromOpcUaValueError};
use crate::opcua::traceability::cache::GetGeneralPartSheetError;

use super::initialize::Initialized;
use super::{ReadError, TraceabilityHandler, WriteError};

/// Errors that can occur during handling the request to read the general part sheet.
#[derive(Debug, Error)]
pub(super) enum ReadPartSheetError {
    #[error("error reading the part ID")]
    ReadPartId(#[source] ReadError),
    #[error("invalid part ID value, cause: {0}")]
    PartIdValue(#[source] TryFromOpcUaValueError),
    #[error("error getting general part sheet from cache")]
    CacheGet(#[source] GetGeneralPartSheetError),
    #[error("general part sheet not found for id {0}")]
    CacheMissing(String),
    #[error("error writing the general part sheet to the OPC-UA server")]
    WritePartSheet(#[source] WriteError),
}

impl TraceabilityHandler<Initialized> {
    /// Run the request from the OPC-UA server to read the part sheet, i.e.
    /// read general part data from the cache and write it to the server.
    #[instrument(err, skip_all)]
    pub(super) async fn read_part_sheet(&self) -> Result<(), ReadPartSheetError> {
        // Get the part ID from the OPC-UA server.
        let values = self
            .read_values(&[self.config.nodes.part_id])
            .await
            .map_err(ReadPartSheetError::ReadPartId)?;
        let [part_id_value] = values
            .try_into()
            .expect("read values vector should have the expected size");
        let part_id: &str = part_id_value
            .try_ua_value_as()
            .map_err(ReadPartSheetError::PartIdValue)?;

        // Get and decode the general part sheet from the cache, using a blocking task.
        let part_sheet_from_cache = tokio::task::block_in_place(move || {
            self.cache
                .get_general_part_sheet(part_id, &self.session.encoding_context().read().context())
                .map_err(ReadPartSheetError::CacheGet)
        })?;
        let part_sheet = part_sheet_from_cache
            .ok_or_else(|| ReadPartSheetError::CacheMissing(part_id.to_owned()))?;

        // Write the general part sheet to the server.
        self.write_values(part_sheet)
            .await
            .map_err(ReadPartSheetError::WritePartSheet)?;

        info!(msg = "general part sheet read", part_id);

        Ok(())
    }
}
