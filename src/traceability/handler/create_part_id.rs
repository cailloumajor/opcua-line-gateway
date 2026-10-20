use std::sync::Arc;

use futures_util::TryFutureExt;
use opcua::types::{NodeId, ReadValueId, TimestampsToReturn, WriteValue};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, SessionExt, TryFromOpcUaValueError, WriteError};
use crate::traceability::part_id::{PartIdentifierError, create_part_identifier};

use super::{TraceabilityContext, TraceabilityHandler};

/// Errors that can occur during part ID creation.
#[derive(Debug, Error)]
pub(super) enum CreatePartIdError {
    #[error("part ID creation is not configured for this server")]
    NotConfigured,
    #[error("error reading required variables")]
    ReadVariables(#[source] opcua::types::Error),
    #[error("invalid read length: expected {0}, got {1}")]
    ReadLength(usize, usize),
    #[error("invalid raw part reference value, cause: {0}")]
    PartRefValue(TryFromOpcUaValueError),
    #[error("invalid raw batch value, cause: {0}")]
    BatchValue(TryFromOpcUaValueError),
    #[error("error getting next serial number from cache")]
    NextSerial(#[source] redb::Error),
    #[error("blocking task to get next serial failed: {0}")]
    NextSerialTask(JoinError),
    #[error("error generating the part identifier")]
    PartIdentifier(#[source] PartIdentifierError),
    #[error("error writing the part ID")]
    WritePartId(#[source] WriteError),
}

impl TraceabilityHandler<TraceabilityContext> {
    /// Create the part ID by getting required data from the OPC-UA server and writing back the
    /// generated ID.
    #[instrument(err, skip_all)]
    pub(super) async fn create_part_id(&self) -> Result<(), CreatePartIdError> {
        let line_id = self
            .machine_config
            .line_id
            // Return an error if this instance has no part reference configuration.
            .ok_or(CreatePartIdError::NotConfigured)?;

        // Read and convert needed OPC-UA variables.
        let read_value_ids = &[
            ReadValueId::new_value(NodeId::new(
                self.state.general_part_sheet.ns_index,
                self.common_opcua_config.raw_part_ref_nid,
            )),
            ReadValueId::new_value(NodeId::new(
                self.state.general_part_sheet.ns_index,
                self.common_opcua_config.raw_batch_nid,
            )),
        ];
        let values = self
            .session
            .read(read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(CreatePartIdError::ReadVariables)?;
        let [part_ref_value, batch_value] = values
            .try_into()
            .map_err(|err: Vec<_>| CreatePartIdError::ReadLength(2, err.len()))?;
        let part_ref: String = part_ref_value
            .try_ua_value_as()
            .map_err(CreatePartIdError::PartRefValue)?;
        let batch: AsciiDigitsOrUpper<2> = batch_value
            .try_ua_value_as()
            .map_err(CreatePartIdError::BatchValue)?;

        let today = self.time_provider.now_zoned().date();

        // Get the next serial number using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let task = tokio::task::spawn_blocking(move || sent_cache.next_serial(today));
        let serial = task
            .await
            .map_err(CreatePartIdError::NextSerialTask)?
            .map_err(CreatePartIdError::NextSerial)?;

        // Create the part identifier.
        let part_id = create_part_identifier(&part_ref, batch, line_id, today, serial)
            .map_err(CreatePartIdError::PartIdentifier)?;

        let write_values = &[WriteValue::value_attr(
            NodeId::new(
                self.state.general_part_sheet.ns_index,
                self.common_opcua_config.part_id_nid,
            ),
            part_id.clone().into(),
        )];
        self.session
            .write_checked(write_values)
            .map_err(CreatePartIdError::WritePartId)
            .await?;

        info!(msg = "created part identifier", part_id);

        Ok(())
    }
}
