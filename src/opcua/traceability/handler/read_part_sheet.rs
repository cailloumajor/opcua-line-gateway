use std::io;

use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::data_value::{DataValueExt, TryFromDataValueError};

use super::initialize::Initialized;
use super::{ReadError, TraceabilityHandler, WriteError};

/// Errors that can occur during handling the request to read the general part sheet.
#[derive(Debug, Error)]
pub(super) enum ReadPartSheetError {
    #[error("error reading the part ID")]
    ReadPartId(#[source] ReadError),
    #[error("invalid part ID value, cause: {0}")]
    PartIdValue(#[source] TryFromDataValueError),
    #[error("error getting general part sheet from cache")]
    CacheGet(#[source] redb::Error),
    #[error("general part sheet not found for id {0}")]
    CacheMissing(String),
    #[error("error decoding the general part sheet")]
    CacheDecode(#[source] io::Error),
    #[error("error joining general part sheet cache retrieval task, cause: {0}")]
    CacheTask(#[source] JoinError),
    #[error("error writing the general part sheet to the OPC-UA server")]
    WritePartSheet(#[source] WriteError),
}

impl TraceabilityHandler<Initialized> {
    /// Run the request from the OPC-UA server to read the part sheet, i.e.
    /// read general part data from the cache and write it to the server.
    #[instrument(err, skip_all, fields(part_id))]
    pub(super) async fn read_part_sheet(&self) -> Result<(), ReadPartSheetError> {
        // Get the part ID from the OPC-UA server.
        let values = self
            .read_values(&[self.config.part_id_node_id])
            .await
            .map_err(ReadPartSheetError::ReadPartId)?;
        let [part_id_value] = values
            .try_into()
            .expect("read values vector should have the expected size");
        let part_id: &str = part_id_value
            .try_as()
            .map_err(ReadPartSheetError::PartIdValue)?;

        // Get and decode the general part sheet from the cache, using a blocking task.
        let sent_cache = self.cache.clone();
        let sent_part_id = part_id.to_owned();
        let sent_ctx = self.session.context();
        let get_and_decode_part_sheet = move || {
            let encoded = sent_cache
                .get_general_part_sheet(&sent_part_id)
                .map_err(ReadPartSheetError::CacheGet)?
                .ok_or(ReadPartSheetError::CacheMissing(sent_part_id))?;
            encoded
                .decode(&sent_ctx.read().context())
                .map_err(ReadPartSheetError::CacheDecode)
        };
        let part_sheet = tokio::task::spawn_blocking(get_and_decode_part_sheet)
            .await
            .map_err(ReadPartSheetError::CacheTask)??;

        // Write the general part sheet to the server.
        self.write_values(part_sheet)
            .await
            .map_err(ReadPartSheetError::WritePartSheet)?;

        info!(msg = "general part sheet read");

        Ok(())
    }
}
