use std::collections::BTreeMap;
use std::sync::Arc;

use opcua::types::{DataValue, NodeId, ReadValueId, TimestampsToReturn, WriteValue};
use opcua_line_gateway_config::{AsciiDigitsOrUpper, NodeName};
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, SessionExt, TryFromOpcUaValueError, WriteError};
use crate::traceability::cache::GetGeneralPartSheetError;
use crate::traceability::part_sheet::SavedPartSheetItem;

use super::{MailboxContext, TraceabilityMailbox};

/// Errors that can occur during handling the request to read the general part sheet.
#[derive(Debug, Error)]
pub(crate) enum HandleReadError {
    #[error("error reading the part ID")]
    ReadPartId(#[source] opcua::types::Error),
    #[error("invalid read length: expected 1, got {0}")]
    ReadLength(usize),
    #[error("invalid part identifier value, cause: {0}")]
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

impl TraceabilityMailbox<MailboxContext> {
    /// Run the request from the OPC-UA server to read the part sheet, i.e.
    /// read general part data from the cache and write it to the server.
    #[instrument(err, skip_all)]
    pub(super) async fn handle_read(&self) -> Result<(), HandleReadError> {
        // Get the part ID from the OPC-UA server.
        let read_value_id = ReadValueId::new_value(self.state.part_id_nid.clone());
        let values = self
            .session
            .read(&[read_value_id], TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(HandleReadError::ReadPartId)?;
        let part_id = parse_part_id(values)?;

        // Get the general part sheet from the cache, using a blocking task.
        let sent_cache = Arc::clone(&self.cache);
        let sent_context = self.session.context();
        let task = tokio::task::spawn_blocking(move || {
            sent_cache.get_general_part_sheet(part_id.as_str(), &sent_context.read().context())
        });
        let part_sheet_from_cache = task
            .await
            .map_err(HandleReadError::CacheGetTask)?
            .map_err(HandleReadError::CacheGet)?;
        let part_sheet = part_sheet_from_cache
            .ok_or_else(|| HandleReadError::CacheMissing(part_id.to_string()))?;

        // Write the general part sheet to the server.
        let write_values = general_part_sheet_write_values(
            part_sheet,
            &self.state.general_part_sheet,
            &self.state.excluded_read_nodes,
        );
        self.session
            .write_checked(&write_values)
            .await
            .map_err(HandleReadError::WritePartSheet)?;

        info!(msg = "general part sheet read", %part_id);

        Ok(())
    }
}

/// Parse the part identifier read from the OPC-UA server.
fn parse_part_id(values: Vec<DataValue>) -> Result<AsciiDigitsOrUpper<23>, HandleReadError> {
    let [part_id_value] = values
        .try_into()
        .map_err(|err: Vec<_>| HandleReadError::ReadLength(err.len()))?;

    part_id_value
        .try_ua_value_as()
        .map_err(HandleReadError::PartIdValue)
}

/// Build the values to write to the OPC-UA server from a general part sheet
/// retrieved from the cache. Nodes configured to be excluded, and nodes unknown
/// to this mailbox general part sheet, are skipped.
fn general_part_sheet_write_values(
    part_sheet: Vec<SavedPartSheetItem>,
    general_part_sheet: &BTreeMap<NodeName, NodeId>,
    excluded_read_nodes: &[NodeName],
) -> Vec<WriteValue> {
    part_sheet
        .into_iter()
        .filter(|(name, _)| !excluded_read_nodes.contains(name))
        .filter_map(|(name, val)| {
            general_part_sheet
                .get(&name)
                .map(|nid| WriteValue::value_attr(nid.clone(), val))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use opcua::types::{AttributeId, Variant};

    use super::*;

    fn name(s: &str) -> NodeName {
        s.to_owned().try_into().expect("node name should be valid")
    }

    mod parse_part_id {
        use super::*;

        const PART_ID: &str = "123456789XX422634300001";

        #[test]
        fn ok() {
            let got = parse_part_id(vec![DataValue::value_only(PART_ID)]).expect("should succeed");

            assert_eq!(got.as_str(), PART_ID);
        }

        #[test]
        fn no_value() {
            let result = parse_part_id(vec![]);

            assert_matches!(result, Err(HandleReadError::ReadLength(0)));
        }

        #[test]
        fn too_many_values() {
            let values = vec![
                DataValue::value_only(PART_ID),
                DataValue::value_only(PART_ID),
            ];

            let result = parse_part_id(values);

            assert_matches!(result, Err(HandleReadError::ReadLength(2)));
        }

        #[test]
        fn invalid_value() {
            let result = parse_part_id(vec![DataValue::value_only("too short")]);

            assert_matches!(result, Err(HandleReadError::PartIdValue(_)));
        }
    }

    mod general_part_sheet_write_values {
        use super::*;

        #[test]
        fn maps_names_to_node_ids() {
            let general = BTreeMap::from([
                (name("a"), NodeId::new(1, 10)),
                (name("b"), NodeId::new(1, 11)),
            ]);
            let part_sheet = vec![
                (name("a"), Variant::from(true)),
                (name("b"), Variant::from(42u16)),
            ];

            let got = general_part_sheet_write_values(part_sheet, &general, &[]);

            assert_eq!(got.len(), 2);
            assert_eq!(got[0].node_id, NodeId::new(1, 10));
            assert_eq!(got[0].attribute_id, AttributeId::Value as u32);
            assert_eq!(got[0].value.value, Some(Variant::from(true)));
            assert_eq!(got[1].node_id, NodeId::new(1, 11));
            assert_eq!(got[1].value.value, Some(Variant::from(42u16)));
        }

        #[test]
        fn skips_excluded_nodes() {
            let general = BTreeMap::from([
                (name("a"), NodeId::new(1, 10)),
                (name("b"), NodeId::new(1, 11)),
            ]);
            let part_sheet = vec![
                (name("a"), Variant::from(true)),
                (name("b"), Variant::from(42u16)),
            ];

            let got = general_part_sheet_write_values(part_sheet, &general, &[name("a")]);

            assert_eq!(got.len(), 1);
            assert_eq!(got[0].node_id, NodeId::new(1, 11));
        }

        #[test]
        fn skips_unknown_nodes() {
            let general = BTreeMap::from([(name("a"), NodeId::new(1, 10))]);
            let part_sheet = vec![
                (name("a"), Variant::from(true)),
                (name("unknown"), Variant::from(42u16)),
            ];

            let got = general_part_sheet_write_values(part_sheet, &general, &[]);

            assert_eq!(got.len(), 1);
            assert_eq!(got[0].node_id, NodeId::new(1, 10));
        }
    }
}
