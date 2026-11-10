use std::collections::BTreeMap;

use opcua::client::Session;
use opcua::types::{
    DataValue, NodeClass, NodeId, QualifiedName, ReadValueId, ReferenceDescription,
    TimestampsToReturn,
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

        let root_node_id = &root_node_ref.node_id.node_id;
        let root_node_refs = children
            .remove(root_node_id)
            .ok_or_else(|| MailboxInitializeError::MailboxMissing(root_node_id.clone()))?;
        let MailboxChildren {
            request_nid,
            response_nid,
            general_part_sheet_nid,
            operation_part_sheet_nid,
        } = find_mailbox_children(root_node_refs, common_ns_index, opcua_config, supertypes)?;

        let general_part_sheet_refs = children
            .remove(&general_part_sheet_nid)
            .ok_or(MailboxInitializeError::GeneralPartSheetMissing)?;
        let general_part_sheet = build_part_sheet(general_part_sheet_refs, &self.session).await?;
        info!(
            msg = "general part sheet nodes discovered",
            count = general_part_sheet.len()
        );

        let GeneralPartSheetNodes {
            part_id_nid,
            raw_part_ref_nid,
            raw_batch_nid,
            excluded_read_nodes,
        } = find_general_part_sheet_nodes(&general_part_sheet, opcua_config)?;

        let operation_part_sheet_refs = children
            .remove(&operation_part_sheet_nid)
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

/// Nodes found as direct children of a mailbox node.
#[derive(Debug)]
struct MailboxChildren {
    /// Request OPC-UA `NodeId`.
    request_nid: NodeId,
    /// Response OPC-UA `NodeId`.
    response_nid: NodeId,
    /// General part sheet object OPC-UA `NodeId`.
    general_part_sheet_nid: NodeId,
    /// Operation part sheet object OPC-UA `NodeId`.
    operation_part_sheet_nid: NodeId,
}

/// Find the expected nodes among the children of a mailbox node. The operation
/// part sheet is identified by its type, whose supertypes are looked up in
/// `supertypes`.
fn find_mailbox_children(
    node_refs: Vec<ReferenceDescription>,
    common_ns_index: u16,
    opcua_config: &TraceabilityCommonOpcUaConfig,
    supertypes: &BrowseResults,
) -> Result<MailboxChildren, MailboxInitializeError> {
    // Whether a `BrowseName` is the given name, in the common namespace.
    let is_common_name = |browse_name: &QualifiedName, name: &str| {
        browse_name.namespace_index == common_ns_index && browse_name.name == name
    };

    let mut request_nid = None;
    let mut response_nid = None;
    let mut general_part_sheet_nid = None;
    let mut operation_part_sheet_nid = None;
    for node_ref in node_refs {
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

    Ok(MailboxChildren {
        request_nid: request_nid.ok_or(MailboxInitializeError::RequestMissing)?,
        response_nid: response_nid.ok_or(MailboxInitializeError::ResponseMissing)?,
        general_part_sheet_nid: general_part_sheet_nid
            .ok_or(MailboxInitializeError::GeneralPartSheetMissing)?,
        operation_part_sheet_nid: operation_part_sheet_nid
            .ok_or(MailboxInitializeError::OperationPartSheetMissing)?,
    })
}

/// Nodes of the general part sheet with a specific role.
#[derive(Debug)]
struct GeneralPartSheetNodes {
    /// Part identifier OPC-UA `NodeId`.
    part_id_nid: NodeId,
    /// Raw part reference OPC-UA `NodeId`.
    raw_part_ref_nid: NodeId,
    /// Raw material batch OPC-UA `NodeId`.
    raw_batch_nid: NodeId,
    /// `NodeName`s of the nodes excluded from writing to the machine.
    excluded_read_nodes: Vec<NodeName>,
}

/// Find the nodes with a specific role in the general part sheet, as configured.
fn find_general_part_sheet_nodes(
    general_part_sheet: &BTreeMap<NodeName, NodeId>,
    opcua_config: &TraceabilityCommonOpcUaConfig,
) -> Result<GeneralPartSheetNodes, MailboxInitializeError> {
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

    Ok(GeneralPartSheetNodes {
        part_id_nid,
        raw_part_ref_nid,
        raw_batch_nid,
        excluded_read_nodes,
    })
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
/// collection of node references. The nodes values are read to ensure they are
/// supported.
async fn build_part_sheet(
    node_refs: Vec<ReferenceDescription>,
    session: &Session,
) -> Result<BTreeMap<NodeName, NodeId>, BuildPartSheetError> {
    let part_sheet = collect_part_sheet_nodes(node_refs)?;

    // Read the nodes values and ensure we can serialize them.
    let read_value_ids = part_sheet
        .values()
        .map(|id| ReadValueId::new_value(id.clone()))
        .collect::<Vec<_>>();
    let values = session
        .read(&read_value_ids, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(BuildPartSheetError::ReadRequest)?;
    check_part_sheet_values(&part_sheet, values)?;

    Ok(part_sheet)
}

/// Collect the variable nodes of a part sheet, from a collection of node
/// references.
fn collect_part_sheet_nodes(
    node_refs: Vec<ReferenceDescription>,
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

    Ok(part_sheet)
}

/// Check that the values read for a part sheet are valid and serializable. The
/// values are expected in the order of the part sheet nodes.
fn check_part_sheet_values(
    part_sheet: &BTreeMap<NodeName, NodeId>,
    values: Vec<DataValue>,
) -> Result<(), BuildPartSheetError> {
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use opcua::types::{Guid, StatusCode, UAString, Variant};

    use crate::opcua::TryFromOpcUaValueError;

    use super::*;

    const NS: u16 = 2;

    fn name(s: &str) -> NodeName {
        s.to_owned().try_into().expect("node name should be valid")
    }

    fn config() -> TraceabilityCommonOpcUaConfig {
        TraceabilityCommonOpcUaConfig {
            namespace_url: "urn:test".to_owned(),
            heartbeat_node: "Heartbeat".to_owned(),
            mailbox_type: "MailboxType".to_owned(),
            request_node: "Request".to_owned(),
            response_node: "Response".to_owned(),
            general_part_sheet_node: "GeneralPartSheet".to_owned(),
            operation_part_sheet_type: "OperationPartSheetType".to_owned(),
            part_id_node: name("PartId"),
            raw_part_ref_node: name("RawPartRef"),
            raw_batch_node: name("RawBatch"),
            excluded_read_nodes: vec![name("Excluded")],
        }
    }

    fn node_ref(
        node_class: NodeClass,
        ns: u16,
        browse_name: &str,
        node_id: NodeId,
    ) -> ReferenceDescription {
        ReferenceDescription {
            node_id: node_id.into(),
            browse_name: QualifiedName::new(ns, browse_name),
            node_class,
            ..Default::default()
        }
    }

    mod find_mailbox_children {
        use super::*;

        /// Machine specific operation part sheet type.
        fn op_type_id() -> NodeId {
            NodeId::new(3, 1000)
        }

        /// Supertypes of the machine specific operation part sheet type.
        fn supertypes() -> BrowseResults {
            BrowseResults::from([(
                op_type_id(),
                vec![node_ref(
                    NodeClass::ObjectType,
                    NS,
                    "OperationPartSheetType",
                    NodeId::new(NS, 500),
                )],
            )])
        }

        fn operation_part_sheet_ref() -> ReferenceDescription {
            ReferenceDescription {
                type_definition: op_type_id().into(),
                ..node_ref(NodeClass::Object, 3, "OP10", NodeId::new(3, 4))
            }
        }

        fn children() -> Vec<ReferenceDescription> {
            vec![
                node_ref(NodeClass::Variable, NS, "Request", NodeId::new(3, 1)),
                node_ref(NodeClass::Variable, NS, "Response", NodeId::new(3, 2)),
                node_ref(NodeClass::Object, NS, "GeneralPartSheet", NodeId::new(3, 3)),
                operation_part_sheet_ref(),
            ]
        }

        #[test]
        fn ok() {
            let mut refs = children();
            // Ignored nodes.
            refs.push(node_ref(
                NodeClass::Variable,
                NS,
                "Other",
                NodeId::new(3, 5),
            ));
            refs.push(node_ref(
                NodeClass::Object,
                NS,
                "Request",
                NodeId::new(3, 6),
            ));

            let got =
                find_mailbox_children(refs, NS, &config(), &supertypes()).expect("should succeed");

            assert_eq!(got.request_nid, NodeId::new(3, 1));
            assert_eq!(got.response_nid, NodeId::new(3, 2));
            assert_eq!(got.general_part_sheet_nid, NodeId::new(3, 3));
            assert_eq!(got.operation_part_sheet_nid, NodeId::new(3, 4));
        }

        #[test]
        fn wrong_namespace() {
            let mut refs = children();
            refs[0] = node_ref(NodeClass::Variable, NS + 1, "Request", NodeId::new(3, 1));

            let result = find_mailbox_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(MailboxInitializeError::RequestMissing));
        }

        #[test]
        fn wrong_node_class() {
            let mut refs = children();
            refs[1] = node_ref(NodeClass::Object, NS, "Response", NodeId::new(3, 2));

            let result = find_mailbox_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(MailboxInitializeError::ResponseMissing));
        }

        #[test]
        fn general_part_sheet_missing() {
            let mut refs = children();
            refs.remove(2);

            let result = find_mailbox_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(MailboxInitializeError::GeneralPartSheetMissing));
        }

        #[test]
        fn operation_part_sheet_unknown_type() {
            let result = find_mailbox_children(children(), NS, &config(), &BrowseResults::new());

            assert_matches!(
                result,
                Err(MailboxInitializeError::OperationPartSheetMissing)
            );
        }
    }

    mod find_general_part_sheet_nodes {
        use super::*;

        fn general_part_sheet() -> BTreeMap<NodeName, NodeId> {
            BTreeMap::from([
                (name("PartId"), NodeId::new(3, 1)),
                (name("RawPartRef"), NodeId::new(3, 2)),
                (name("RawBatch"), NodeId::new(3, 3)),
                (name("Excluded"), NodeId::new(3, 4)),
            ])
        }

        #[test]
        fn ok() {
            let got = find_general_part_sheet_nodes(&general_part_sheet(), &config())
                .expect("should succeed");

            assert_eq!(got.part_id_nid, NodeId::new(3, 1));
            assert_eq!(got.raw_part_ref_nid, NodeId::new(3, 2));
            assert_eq!(got.raw_batch_nid, NodeId::new(3, 3));
            assert_eq!(got.excluded_read_nodes, [name("Excluded")]);
        }

        macro_rules! missing_test {
            ($name:ident, $node:literal, $err:pat) => {
                #[test]
                fn $name() {
                    let mut part_sheet = general_part_sheet();
                    part_sheet.remove($node);

                    let result = find_general_part_sheet_nodes(&part_sheet, &config());

                    assert_matches!(result, Err($err));
                }
            };
        }

        missing_test!(
            part_id_missing,
            "PartId",
            MailboxInitializeError::PartIdMissing
        );
        missing_test!(
            raw_part_ref_missing,
            "RawPartRef",
            MailboxInitializeError::RawPartRefMissing
        );
        missing_test!(
            raw_batch_missing,
            "RawBatch",
            MailboxInitializeError::RawBatchMissing
        );
        missing_test!(
            excluded_missing,
            "Excluded",
            MailboxInitializeError::ExcludedNodeMissing(_)
        );
    }

    mod collect_part_sheet_nodes {
        use super::*;

        #[test]
        fn ok() {
            let refs = vec![
                node_ref(NodeClass::Variable, 3, "b", NodeId::new(3, 2)),
                node_ref(NodeClass::Variable, 3, "a", NodeId::new(3, 1)),
                node_ref(NodeClass::Object, 3, "ignored", NodeId::new(3, 3)),
            ];

            let got = collect_part_sheet_nodes(refs).expect("should succeed");

            assert_eq!(
                got,
                BTreeMap::from([
                    (name("a"), NodeId::new(3, 1)),
                    (name("b"), NodeId::new(3, 2))
                ])
            );
        }

        #[test]
        fn empty() {
            let refs = vec![node_ref(NodeClass::Object, 3, "ignored", NodeId::new(3, 3))];

            let result = collect_part_sheet_nodes(refs);

            assert_matches!(result, Err(BuildPartSheetError::EmptyBrowse));
        }

        #[test]
        fn null_browse_name() {
            let refs = vec![ReferenceDescription {
                browse_name: QualifiedName {
                    namespace_index: 3,
                    name: UAString::null(),
                },
                ..node_ref(NodeClass::Variable, 3, "", NodeId::new(3, 1))
            }];

            let result = collect_part_sheet_nodes(refs);

            assert_matches!(result, Err(BuildPartSheetError::NullBrowseName(id)) if id == NodeId::new(3, 1));
        }

        #[test]
        fn invalid_node_name() {
            let refs = vec![node_ref(
                NodeClass::Variable,
                3,
                &"x".repeat(256),
                NodeId::new(3, 1),
            )];

            let result = collect_part_sheet_nodes(refs);

            assert_matches!(result, Err(BuildPartSheetError::NodeName(_)));
        }
    }

    mod check_part_sheet_values {
        use super::*;

        fn part_sheet() -> BTreeMap<NodeName, NodeId> {
            BTreeMap::from([
                (name("a"), NodeId::new(3, 1)),
                (name("b"), NodeId::new(3, 2)),
            ])
        }

        #[test]
        fn ok() {
            let values = vec![DataValue::value_only(true), DataValue::value_only("b")];

            check_part_sheet_values(&part_sheet(), values).expect("should succeed");
        }

        #[test]
        fn values_count() {
            let values = vec![DataValue::value_only(true)];

            let result = check_part_sheet_values(&part_sheet(), values);

            assert_matches!(result, Err(BuildPartSheetError::ValuesCount(2, 1)));
        }

        #[test]
        fn invalid_data_value() {
            let values = vec![
                DataValue::value_only(true),
                DataValue {
                    status: Some(StatusCode::BadNodeIdUnknown),
                    ..Default::default()
                },
            ];

            let result = check_part_sheet_values(&part_sheet(), values);

            assert_matches!(
                result,
                Err(BuildPartSheetError::InvalidDataValue(
                    TryFromOpcUaValueError::BadStatus(StatusCode::BadNodeIdUnknown),
                    node
                )) if node == "b"
            );
        }

        #[test]
        fn not_serializable() {
            let values = vec![
                DataValue::value_only(Variant::from(Guid::new())),
                DataValue::value_only("b"),
            ];

            let result = check_part_sheet_values(&part_sheet(), values);

            assert_matches!(
                result,
                Err(BuildPartSheetError::NotSerializable(_, node)) if node == "a"
            );
        }
    }
}
