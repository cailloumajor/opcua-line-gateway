use futures_util::TryFutureExt;
use jiff::Timestamp;
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use thiserror::Error;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};
use crate::timezone::system_timezone;
use crate::traceability::part_id::{PartIdentifierError, create_part_identifier};

use super::{ReadError, TraceabilityContext, TraceabilityHandler, WriteError};

/// Errors that can occur during part ID creation.
#[derive(Debug, Error)]
pub(super) enum CreatePartIdError {
    #[error("part ID creation is not configured for this server")]
    NotConfigured,
    #[error("error reading required variables")]
    ReadVariables(#[source] ReadError),
    #[error("invalid raw part reference value, cause: {0}")]
    PartRefValue(TryFromOpcUaValueError),
    #[error("invalid raw batch value, cause: {0}")]
    BatchValue(TryFromOpcUaValueError),
    #[error("error getting next serial number from cache")]
    NextSerial(#[source] redb::Error),
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
        let config = self
            .config
            .part_identifier
            .as_ref()
            // Return an error if this instance has no part reference configuration.
            .ok_or(CreatePartIdError::NotConfigured)?;

        // Read and convert needed OPC-UA variables.
        let values = self
            .read_values([config.raw_part_ref_node, config.raw_batch_node])
            .await
            .map_err(CreatePartIdError::ReadVariables)?;
        let [part_ref_value, batch_value] = values
            .try_into()
            .expect("read values vector should have the expected size");
        let part_ref: String = part_ref_value
            .try_ua_value_as()
            .map_err(CreatePartIdError::PartRefValue)?;
        let batch: AsciiDigitsOrUpper<2> = batch_value
            .try_ua_value_as()
            .map_err(CreatePartIdError::BatchValue)?;

        let today = Timestamp::now().to_zoned(system_timezone().clone()).date();

        // Get the next serial number using a blocking task.
        let serial = tokio::task::block_in_place(move || {
            self.cache
                .next_serial(today)
                .map_err(CreatePartIdError::NextSerial)
        })?;

        // Create the part identifier.
        let part_id = create_part_identifier(&part_ref, batch, config.line_id, today, serial)
            .map_err(CreatePartIdError::PartIdentifier)?;

        self.write_values([(self.config.nodes.part_id, part_id.clone().into())])
            .map_err(CreatePartIdError::WritePartId)
            .await?;

        info!(msg = "created part identifier", part_id);

        Ok(())
    }
}
