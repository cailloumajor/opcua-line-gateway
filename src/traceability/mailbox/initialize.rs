use std::collections::BTreeMap;

use opcua::client::Session;
use opcua::types::{
    NodeClass, NodeId, QualifiedName, ReadValueId, ReferenceDescription, TimestampsToReturn,
};
use opcua_line_gateway_config::{NodeName, NodeNameError, TraceabilityCommonOpcUaConfig};
use thiserror::Error;
use tracing::{Span, debug, info, instrument, warn};

use crate::opcua::{DataValueExt, SerializeVariant, TryFromOpcUaValueError};
use crate::traceability::handler::BrowseResults;

use super::{MailboxContext, TraceabilityMailbox};

/// Errors that can occur during mailbox initialization.
#[derive(Debug, Error)]
pub(crate) enum MailboxInitializeError {
    #[error("mailbox node {0} not found in browsed nodes")]
    MailboxMissing(NodeId),
    #[error("request node not found")]
    RequestMissing,
    #[error("response node not found")]
    ResponseMissing,
    #[error("general part sheet node not found")]
    GeneralPartSheetMissing,
    #[error("operation part sheet node not found")]
    OperationPartSheetMissing,
    #[error("part identifier node not found")]
    PartIdMissing,
    #[error("raw part reference node not found")]
    RawPartRefMissing,
    #[error("raw material batch node not found")]
    RawBatchMissing,
    #[error("node to exclude not found: {0}")]
    ExcludedNodeMissing(NodeName),
    #[error("error building part sheet: {0}")]
    BuildPartSheet(#[from] BuildPartSheetError),
}

impl TraceabilityMailbox<()> {
    /// Initialize the mailbox. This involves interacting with the session.
    #[instrument(name = "mailbox_initialize", err, skip_all, fields(mailbox))]
    pub(crate) async fn initialize(
        self,
        common_ns_index: u16,
        opcua_config: &TraceabilityCommonOpcUaConfig,
        children: &mut BrowseResults,
        supertypes: &BrowseResults,
        root_node_ref: ReferenceDescription,
    ) -> Result<TraceabilityMailbox<MailboxContext>, MailboxInitializeError> {
        Span::current().record("mailbox", root_node_ref.browse_name.name.as_ref());

        // Whether a `BrowseName` is the given name, in the common namespace.
        let is_common_name = |browse_name: &QualifiedName, name: &str| {
            browse_name.namespace_index == common_ns_index && browse_name.name == name
        };

        let root_node_id = &root_node_ref.node_id.node_id;
        let root_node_refs = children
            .remove(root_node_id)
            .ok_or_else(|| MailboxInitializeError::MailboxMissing(root_node_id.clone()))?;

        let mut request_nid = None;
        let mut response_nid = None;
        let mut general_part_sheet_nid = None;
        let mut operation_part_sheet_nid = None;
        for node_ref in root_node_refs {
            let browse_name = &node_ref.browse_name;
            let node_id = node_ref.node_id.node_id;
            let is_operation_part_sheet = || {
                supertypes
                    .get(&node_ref.type_definition.node_id)
                    .is_some_and(|refs| {
                        refs.iter().any(|r| {
                            is_common_name(&r.browse_name, &opcua_config.operation_part_sheet_type)
                        })
                    })
            };

            match node_ref.node_class {
                NodeClass::Variable if is_common_name(browse_name, &opcua_config.request_node) => {
                    request_nid = Some(node_id);
                }
                NodeClass::Variable if is_common_name(browse_name, &opcua_config.response_node) => {
                    response_nid = Some(node_id);
                }
                NodeClass::Object
                    if is_common_name(browse_name, &opcua_config.general_part_sheet_node) =>
                {
                    general_part_sheet_nid = Some(node_id);
                }
                NodeClass::Object if is_operation_part_sheet() => {
                    operation_part_sheet_nid = Some(node_id);
                }
                _ => {
                    debug!(msg = "ignored mailbox node child", %browse_name);
                }
            }
        }
        let request_nid = request_nid.ok_or(MailboxInitializeError::RequestMissing)?;
        let response_nid = response_nid.ok_or(MailboxInitializeError::ResponseMissing)?;

        let general_part_sheet_refs = general_part_sheet_nid
            .and_then(|id| children.remove(&id))
            .ok_or(MailboxInitializeError::GeneralPartSheetMissing)?;
        let general_part_sheet = build_part_sheet(general_part_sheet_refs, &self.session).await?;
        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet.len()
        );

        let part_id_nid = general_part_sheet
            .get(&opcua_config.part_id_node)
            .cloned()
            .ok_or(MailboxInitializeError::PartIdMissing)?;
        let raw_part_ref_nid = general_part_sheet
            .get(&opcua_config.raw_part_ref_node)
            .cloned()
            .ok_or(MailboxInitializeError::RawPartRefMissing)?;
        let raw_batch_nid = general_part_sheet
            .get(&opcua_config.raw_batch_node)
            .cloned()
            .ok_or(MailboxInitializeError::RawBatchMissing)?;

        let excluded_read_nodes = opcua_config
            .excluded_read_nodes
            .iter()
            .map(|name| {
                if general_part_sheet.contains_key(name) {
                    Ok(name.clone())
                } else {
                    Err(MailboxInitializeError::ExcludedNodeMissing(name.clone()))
                }
            })
            .collect::<Result<_, _>>()?;

        let operation_part_sheet_refs = operation_part_sheet_nid
            .and_then(|id| children.remove(&id))
            .ok_or(MailboxInitializeError::OperationPartSheetMissing)?;
        let operation_part_sheet =
            build_part_sheet(operation_part_sheet_refs, &self.session).await?;
        info!(
            msg = "operation part sheet nodes discovered",
            count = operation_part_sheet.len()
        );

        let state = MailboxContext {
            request_is_initial: true,
            request_nid,
            response_nid,
            part_id_nid,
            raw_part_ref_nid,
            raw_batch_nid,
            general_part_sheet,
            excluded_read_nodes,
            operation_part_sheet,
        };

        Ok(TraceabilityMailbox {
            line_id: self.line_id,
            machine_id: self.machine_id,
            time_provider: self.time_provider,
            cache: self.cache,
            session: self.session,
            state,
        })
    }
}

/// Errors that can occur when building a part sheet.
#[derive(Debug, Error)]
pub(crate) enum BuildPartSheetError {
    #[error("unexpected null browse name for node {0}")]
    NullBrowseName(NodeId),
    #[error("invalid node name: {0}")]
    NodeName(#[source] NodeNameError),
    #[error("no node found when browsing the part sheet")]
    EmptyBrowse,
    #[error("error with OPC-UA read request")]
    ReadRequest(#[source] opcua::types::Error),
    #[error("invalid number of read variables (discovered {0}, read {1})")]
    ValuesCount(usize, usize),
    #[error("invalid discovered data value for node {1}, cause: {0}")]
    InvalidDataValue(TryFromOpcUaValueError, String),
    #[error("unsupported Variant for serialization for node {1}, cause: {0}")]
    NotSerializable(serde_json::Error, String),
}

/// Build a mapping of node name to NodeID, representing a part sheet, from a
/// collection of node references.
async fn build_part_sheet(
    node_refs: Vec<ReferenceDescription>,
    session: &Session,
) -> Result<BTreeMap<NodeName, NodeId>, BuildPartSheetError> {
    let mut part_sheet = BTreeMap::new();

    for node_ref in node_refs {
        let browse_name = &node_ref.browse_name;
        let node_id = node_ref.node_id.node_id;

        if node_ref.node_class == NodeClass::Variable {
            if !node_id.is_numeric() {
                warn!(msg = "non-numeric NodeID found", %node_id);
            }
            let Some(browse_name) = browse_name.name.value().clone() else {
                return Err(BuildPartSheetError::NullBrowseName(node_id));
            };
            let node_name =
                NodeName::try_from(browse_name).map_err(BuildPartSheetError::NodeName)?;
            part_sheet.insert(node_name, node_id);
        } else {
            debug!(msg = "ignored part sheet node child", %browse_name);
        }
    }

    // If browsing the part sheet did not find any node, do not continue with
    // read request, which would fail with a less contextual error.
    if part_sheet.is_empty() {
        return Err(BuildPartSheetError::EmptyBrowse);
    }

    // Read the nodes values and ensure we can serialize them.
    let read_value_ids = part_sheet
        .values()
        .map(|id| ReadValueId::new_value(id.clone()))
        .collect::<Vec<_>>();
    let values = session
        .read(&read_value_ids, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(BuildPartSheetError::ReadRequest)?;
    let expected_len = part_sheet.len();
    let got_len = values.len();
    if got_len != expected_len {
        return Err(BuildPartSheetError::ValuesCount(expected_len, got_len));
    }
    for (value, name) in values.into_iter().zip(part_sheet.keys()) {
        let variant = value
            .try_into_variant()
            .map_err(|err| BuildPartSheetError::InvalidDataValue(err, name.to_string()))?;
        serde_json::to_value(SerializeVariant(&variant))
            .map_err(|err| BuildPartSheetError::NotSerializable(err, name.to_string()))?;
    }

    Ok(part_sheet)
}
