use std::collections::BTreeMap;
use std::sync::Arc;

use opcua::types::{DataValue, NodeId, ReadValueId, TimestampsToReturn};
use opcua_line_gateway_config::{AsciiDigitsOrUpper, NodeName};
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};
use crate::traceability::cache::{CheckEnqueuingError, SavePartSheetsError};
use crate::traceability::part_id::{PartIdentifierError, validate_part_identifier};
use crate::traceability::part_sheet::SavedPartSheetItem;

use super::{MailboxContext, TraceabilityMailbox};

/// Errors that can occur during handling the request for saving part sheets.
#[derive(Debug, Error)]
pub(crate) enum HandleSaveError {
    #[error("error reading part sheets nodes")]
    ReadPartSheets(#[source] opcua::types::Error),
    #[error("invalid number of read variables (expected {0}, got {1})")]
    ReadLength(usize, usize),
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

impl TraceabilityMailbox<MailboxContext> {
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

        let read_value_ids = part_sheets_read_value_ids(
            &self.state.general_part_sheet,
            &self.state.operation_part_sheet,
            &self.state.part_id_nid,
        );

        // Read nodes values from the server.
        let read_values = self
            .session
            .read(&read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(HandleSaveError::ReadPartSheets)?;

        let ReadPartSheets {
            part_id,
            general_part_sheet,
            operation_part_sheet,
        } = parse_part_sheets(
            read_values,
            &self.state.general_part_sheet,
            &self.state.operation_part_sheet,
        )?;

        // Insert the part sheets in the cache, using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let sent_machine_id = Arc::clone(&self.machine_id);
        let sent_context = self.session.context();
        let task = tokio::task::spawn_blocking(move || {
            sent_cache.save_part_sheets(
                sent_machine_id,
                part_id,
                &general_part_sheet,
                &operation_part_sheet,
                &sent_context.read().context(),
            )
        });

        task.await
            .map_err(HandleSaveError::CacheSaveTask)?
            .map_err(HandleSaveError::CacheSave)?;

        info!(msg = "part sheets saved", %part_id);

        Ok(())
    }
}

/// Build the list of elements to read from the OPC-UA server to save the part
/// sheets, i.e. general part sheet nodes, operation part sheet nodes and part
/// identifier node, in this order. Read values must be parsed with
/// [`parse_part_sheets`].
fn part_sheets_read_value_ids(
    general_part_sheet: &BTreeMap<NodeName, NodeId>,
    operation_part_sheet: &BTreeMap<NodeName, NodeId>,
    part_id_nid: &NodeId,
) -> Vec<ReadValueId> {
    general_part_sheet
        .values()
        .chain(operation_part_sheet.values())
        .chain([part_id_nid])
        .map(|id| ReadValueId::new_value(id.clone()))
        .collect()
}

/// Part sheets read from the OPC-UA server.
#[derive(Debug)]
struct ReadPartSheets {
    /// The part identifier.
    part_id: AsciiDigitsOrUpper<23>,
    /// The general part sheet.
    general_part_sheet: Vec<SavedPartSheetItem>,
    /// The operation part sheet.
    operation_part_sheet: Vec<SavedPartSheetItem>,
}

/// Parse the values read from the OPC-UA server, in the order given by
/// [`part_sheets_read_value_ids`].
fn parse_part_sheets(
    mut read_values: Vec<DataValue>,
    general_part_sheet: &BTreeMap<NodeName, NodeId>,
    operation_part_sheet: &BTreeMap<NodeName, NodeId>,
) -> Result<ReadPartSheets, HandleSaveError> {
    // Ensure we have as many read nodes as requested.
    let expected_len = general_part_sheet.len() + operation_part_sheet.len() + 1;
    let got_len = read_values.len();
    if got_len != expected_len {
        return Err(HandleSaveError::ReadLength(expected_len, got_len));
    }

    // Get the part identifier from the last read element.
    let part_id: AsciiDigitsOrUpper<23> = read_values
        .pop()
        .expect("read values should at least contain the part identifier")
        .try_ua_value_as()
        .map_err(HandleSaveError::PartIdValue)?;
    validate_part_identifier(part_id)
        .map_err(|err| HandleSaveError::InvalidPartId(err, part_id.to_string()))?;

    // Build the general part sheet and the operation part sheet, contiguous
    // in the same collection.
    let mut part_sheets = general_part_sheet
        .keys()
        .chain(operation_part_sheet.keys())
        .zip(read_values)
        .map(|(name, val)| {
            val.try_into_variant()
                .map(|variant| (name.clone(), variant))
                .map_err(|err| HandleSaveError::PartSheetValue(err, name.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Get the operation part sheet out of the collection. Remaining elements
    // after splitting off the operation part sheet are general part sheet
    // elements.
    let operation_part_sheet = part_sheets.split_off(general_part_sheet.len());
    let general_part_sheet = part_sheets;

    Ok(ReadPartSheets {
        part_id,
        general_part_sheet,
        operation_part_sheet,
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use opcua::types::{StatusCode, Variant};

    use crate::opcua::TryFromOpcUaValueError;

    use super::*;

    const PART_ID: &str = "123456789XX422634300001";

    fn name(s: &str) -> NodeName {
        s.to_owned().try_into().expect("node name should be valid")
    }

    fn general() -> BTreeMap<NodeName, NodeId> {
        BTreeMap::from([
            (name("g1"), NodeId::new(1, 10)),
            (name("g2"), NodeId::new(1, 11)),
        ])
    }

    fn operation() -> BTreeMap<NodeName, NodeId> {
        BTreeMap::from([(name("o1"), NodeId::new(1, 20))])
    }

    fn read_values() -> Vec<DataValue> {
        vec![
            DataValue::value_only(true),
            DataValue::value_only(42u16),
            DataValue::value_only("operation"),
            DataValue::value_only(PART_ID),
        ]
    }

    #[test]
    fn read_value_ids_order() {
        let got = part_sheets_read_value_ids(&general(), &operation(), &NodeId::new(1, 99));

        let node_ids = got.into_iter().map(|r| r.node_id).collect::<Vec<_>>();
        assert_eq!(
            node_ids,
            [
                NodeId::new(1, 10),
                NodeId::new(1, 11),
                NodeId::new(1, 20),
                NodeId::new(1, 99)
            ]
        );
    }

    mod parse_part_sheets {
        use super::*;

        #[test]
        fn ok() {
            let got =
                parse_part_sheets(read_values(), &general(), &operation()).expect("should succeed");

            assert_eq!(got.part_id.as_str(), PART_ID);
            assert_eq!(
                got.general_part_sheet,
                [
                    (name("g1"), Variant::from(true)),
                    (name("g2"), Variant::from(42u16))
                ]
            );
            assert_eq!(
                got.operation_part_sheet,
                [(name("o1"), Variant::from("operation"))]
            );
        }

        #[test]
        fn too_few_values() {
            let mut values = read_values();
            values.remove(0);

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(result, Err(HandleSaveError::ReadLength(4, 3)));
        }

        #[test]
        fn too_many_values() {
            let mut values = read_values();
            values.push(DataValue::value_only(PART_ID));

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(result, Err(HandleSaveError::ReadLength(4, 5)));
        }

        #[test]
        fn invalid_part_id_value() {
            let mut values = read_values();
            *values.last_mut().unwrap() = DataValue::value_only(42u8);

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(
                result,
                Err(HandleSaveError::PartIdValue(
                    TryFromOpcUaValueError::InvalidType(..)
                ))
            );
        }

        #[test]
        fn invalid_part_id() {
            let mut values = read_values();
            *values.last_mut().unwrap() = DataValue::value_only("1234A6789XX422634300001");

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(result, Err(HandleSaveError::InvalidPartId(_, id)) if id == "1234A6789XX422634300001");
        }

        #[test]
        fn bad_part_sheet_value() {
            let mut values = read_values();
            values[2] = DataValue {
                status: Some(StatusCode::BadNodeIdUnknown),
                ..Default::default()
            };

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(
                result,
                Err(HandleSaveError::PartSheetValue(
                    TryFromOpcUaValueError::BadStatus(StatusCode::BadNodeIdUnknown),
                    node
                )) if node == "o1"
            );
        }

        #[test]
        fn missing_part_sheet_value() {
            let mut values = read_values();
            values[0] = DataValue::null();

            let result = parse_part_sheets(values, &general(), &operation());

            assert_matches!(
                result,
                Err(HandleSaveError::PartSheetValue(
                    TryFromOpcUaValueError::MissingValue,
                    node
                )) if node == "g1"
            );
        }
    }
}
