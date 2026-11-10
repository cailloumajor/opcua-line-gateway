use std::sync::Arc;

use futures_util::TryFutureExt;
use opcua::types::{DataValue, ReadValueId, TimestampsToReturn, WriteValue};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, SessionExt, TryFromOpcUaValueError, WriteError};
use crate::traceability::part_id::{PartIdentifierError, create_part_identifier};

use super::{MailboxContext, TraceabilityMailbox};

/// Errors that can occur during part ID creation.
#[derive(Debug, Error)]
pub(crate) enum CreatePartIdError {
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

impl TraceabilityMailbox<MailboxContext> {
    /// Create the part ID by getting required data from the OPC-UA server and writing back the
    /// generated ID.
    #[instrument(err, skip_all)]
    pub(super) async fn create_part_id(&self) -> Result<(), CreatePartIdError> {
        // Read and convert needed OPC-UA variables.
        let read_value_ids = &[
            ReadValueId::new_value(self.state.raw_part_ref_nid.clone()),
            ReadValueId::new_value(self.state.raw_batch_nid.clone()),
        ];
        let values = self
            .session
            .read(read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(CreatePartIdError::ReadVariables)?;
        let (part_ref, batch) = parse_part_id_inputs(values)?;

        let today = self.time_provider.now_zoned().date();

        // Get the next serial number using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let task = tokio::task::spawn_blocking(move || sent_cache.next_serial(today));
        let serial = task
            .await
            .map_err(CreatePartIdError::NextSerialTask)?
            .map_err(CreatePartIdError::NextSerial)?;

        // Create the part identifier.
        let part_id = create_part_identifier(&part_ref, batch, self.line_id, today, serial)
            .map_err(CreatePartIdError::PartIdentifier)?;

        let write_values = &[WriteValue::value_attr(
            self.state.part_id_nid.clone(),
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

/// Parse the values read from the OPC-UA server to create the part ID, i.e. the
/// raw part reference and the raw material batch, in this order.
fn parse_part_id_inputs(
    values: Vec<DataValue>,
) -> Result<(String, AsciiDigitsOrUpper<2>), CreatePartIdError> {
    let [part_ref_value, batch_value] = values
        .try_into()
        .map_err(|err: Vec<_>| CreatePartIdError::ReadLength(2, err.len()))?;
    let part_ref = part_ref_value
        .try_ua_value_as()
        .map_err(CreatePartIdError::PartRefValue)?;
    let batch = batch_value
        .try_ua_value_as()
        .map_err(CreatePartIdError::BatchValue)?;

    Ok((part_ref, batch))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use crate::opcua::TryFromOpcUaValueError;

    use super::*;

    mod parse_part_id_inputs {
        use super::*;

        #[test]
        fn ok() {
            let values = vec![
                DataValue::value_only("P89-4865ABCD513A-X846"),
                DataValue::value_only("A1"),
            ];

            let (part_ref, batch) = parse_part_id_inputs(values).expect("should succeed");

            assert_eq!(part_ref, "P89-4865ABCD513A-X846");
            assert_eq!(batch.as_str(), "A1");
        }

        #[test]
        fn too_few_values() {
            let values = vec![DataValue::value_only("P89-4865ABCD513A-X846")];

            let result = parse_part_id_inputs(values);

            assert_matches!(result, Err(CreatePartIdError::ReadLength(2, 1)));
        }

        #[test]
        fn too_many_values() {
            let values = vec![
                DataValue::value_only("P89-4865ABCD513A-X846"),
                DataValue::value_only("A1"),
                DataValue::value_only("A1"),
            ];

            let result = parse_part_id_inputs(values);

            assert_matches!(result, Err(CreatePartIdError::ReadLength(2, 3)));
        }

        #[test]
        fn invalid_part_ref() {
            let values = vec![DataValue::value_only(42u8), DataValue::value_only("A1")];

            let result = parse_part_id_inputs(values);

            assert_matches!(
                result,
                Err(CreatePartIdError::PartRefValue(
                    TryFromOpcUaValueError::InvalidType(..)
                ))
            );
        }

        #[test]
        fn invalid_batch() {
            let values = vec![
                DataValue::value_only("P89-4865ABCD513A-X846"),
                DataValue::value_only("a1"),
            ];

            let result = parse_part_id_inputs(values);

            assert_matches!(result, Err(CreatePartIdError::BatchValue(_)));
        }
    }
}
