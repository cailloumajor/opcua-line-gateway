use std::sync::Arc;

use opcua::types::{NodeClass, NodeId, ReferenceDescription};
use opcua_line_gateway_config::{
    TraceabilityCommonConfig, TraceabilityCommonOpcUaConfig, TraceabilityMachineConfig,
};
use thiserror::Error;
use tracing::{debug, instrument};

use crate::traceability::mailbox::{MailboxInitializeError, TraceabilityMailbox};

use super::{
    BrowseMachineError, BrowseResults, HandlerContext, TraceabilityHandler, has_common_supertype,
};

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {
    #[error("error getting common traceability namespace index")]
    GetCommonNamespaceIndex(#[source] opcua::types::Error),
    #[error("error getting operation traceability namespace index")]
    GetOperationNamespaceIndex(#[source] opcua::types::Error),
    #[error("error browsing the machine object")]
    BrowseMachine(#[source] BrowseMachineError),
    #[error("missing machine root object in browsed nodes")]
    MissingMachine,
    #[error("heartbeat node not found")]
    MissingHeartbeat,
    #[error("no mailbox found")]
    MissingMailboxes,
    #[error("error initializing mailbox")]
    MailboxInitialize(#[source] MailboxInitializeError),
}

impl TraceabilityHandler<()> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_init", err, skip_all)]
    pub(crate) async fn initialize(
        self,
        common_config: &TraceabilityCommonConfig,
        machine_config: &TraceabilityMachineConfig,
    ) -> Result<TraceabilityHandler<HandlerContext>, TraceabilityInitializeError> {
        // Get the namespace index for traceability common part.
        let common_ns_index = self
            .session
            .get_namespace_index(&common_config.opc_ua.namespace_url)
            .await
            .map_err(TraceabilityInitializeError::GetCommonNamespaceIndex)?;

        // Get the namespace index for traceability operation part.
        let operation_ns_index = self
            .session
            .get_namespace_index(&machine_config.namespace_url)
            .await
            .map_err(TraceabilityInitializeError::GetOperationNamespaceIndex)?;

        let machine_node = NodeId::new(operation_ns_index, machine_config.machine_nid);

        let (mut children, supertypes) = self
            .browse_machine(machine_node.clone())
            .await
            .map_err(TraceabilityInitializeError::BrowseMachine)?;

        // Get the references of the root machine node children.
        let machine_children = children
            .remove(&machine_node)
            .ok_or(TraceabilityInitializeError::MissingMachine)?;

        // Extract heartbeat and mailboxes nodes.
        let MachineChildren {
            heartbeat_nid,
            mailbox_refs,
        } = find_machine_children(
            machine_children,
            common_ns_index,
            &common_config.opc_ua,
            &supertypes,
        )?;

        let mut mailboxes = Vec::with_capacity(mailbox_refs.len());
        for mailbox_ref in mailbox_refs {
            let mailbox = TraceabilityMailbox::new(
                common_config.line_id,
                Arc::clone(&self.machine_id),
                self.time_provider.clone(),
                Arc::clone(&self.cache),
                Arc::clone(&self.session),
            )
            .initialize(
                common_ns_index,
                &common_config.opc_ua,
                &mut children,
                &supertypes,
                mailbox_ref,
            )
            .await
            .map_err(TraceabilityInitializeError::MailboxInitialize)?;

            mailboxes.push(mailbox);
        }

        let state = HandlerContext {
            heartbeat: heartbeat_nid,
            mailboxes,
        };

        Ok(TraceabilityHandler {
            machine_id: self.machine_id,
            session: self.session,
            cache: self.cache,
            time_provider: self.time_provider,
            state,
        })
    }
}

/// Nodes found as direct children of a machine node.
#[derive(Debug)]
struct MachineChildren {
    /// Heartbeat OPC-UA `NodeId`.
    heartbeat_nid: NodeId,
    /// References of the mailbox nodes.
    mailbox_refs: Vec<ReferenceDescription>,
}

/// Find the expected nodes among the children of a machine node. Mailboxes are
/// identified by their type, whose supertypes are looked up in `supertypes`.
fn find_machine_children(
    node_refs: Vec<ReferenceDescription>,
    common_ns_index: u16,
    opcua_config: &TraceabilityCommonOpcUaConfig,
    supertypes: &BrowseResults,
) -> Result<MachineChildren, TraceabilityInitializeError> {
    let mut heartbeat_nid = None;
    let mut mailbox_refs = Vec::new();
    for node_ref in node_refs {
        let browse_name = &node_ref.browse_name;
        let is_heartbeat = || {
            browse_name.namespace_index == common_ns_index
                && browse_name.name == opcua_config.heartbeat_node.as_str()
        };
        let is_mailbox = || {
            has_common_supertype(
                supertypes,
                &node_ref.type_definition.node_id,
                common_ns_index,
                &opcua_config.mailbox_type,
            )
        };

        match node_ref.node_class {
            NodeClass::Variable if is_heartbeat() => {
                heartbeat_nid = Some(node_ref.node_id.node_id);
            }
            NodeClass::Object if is_mailbox() => {
                mailbox_refs.push(node_ref);
            }
            _ => {
                debug!(msg = "ignored machine node child", %browse_name);
            }
        }
    }

    let heartbeat_nid = heartbeat_nid.ok_or(TraceabilityInitializeError::MissingHeartbeat)?;
    if mailbox_refs.is_empty() {
        return Err(TraceabilityInitializeError::MissingMailboxes);
    }

    Ok(MachineChildren {
        heartbeat_nid,
        mailbox_refs,
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use opcua::types::QualifiedName;
    use opcua_line_gateway_config::NodeName;

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
            excluded_read_nodes: Vec::new(),
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

    mod find_machine_children {
        use super::*;

        /// Machine specific mailbox type.
        fn mailbox_type_id() -> NodeId {
            NodeId::new(3, 1000)
        }

        /// Supertypes of the machine specific mailbox type.
        fn supertypes() -> BrowseResults {
            BrowseResults::from([(
                mailbox_type_id(),
                vec![node_ref(
                    NodeClass::ObjectType,
                    NS,
                    "MailboxType",
                    NodeId::new(NS, 500),
                )],
            )])
        }

        fn mailbox_ref(browse_name: &str, node_id: NodeId) -> ReferenceDescription {
            ReferenceDescription {
                type_definition: mailbox_type_id().into(),
                ..node_ref(NodeClass::Object, 3, browse_name, node_id)
            }
        }

        fn children() -> Vec<ReferenceDescription> {
            vec![
                node_ref(NodeClass::Variable, NS, "Heartbeat", NodeId::new(3, 1)),
                mailbox_ref("MailboxIn", NodeId::new(3, 2)),
                mailbox_ref("MailboxOut", NodeId::new(3, 3)),
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
                NodeId::new(3, 4),
            ));
            refs.push(node_ref(
                NodeClass::Object,
                NS,
                "Heartbeat",
                NodeId::new(3, 5),
            ));
            refs.push(node_ref(
                NodeClass::Object,
                3,
                "NotAMailbox",
                NodeId::new(3, 6),
            ));

            let got =
                find_machine_children(refs, NS, &config(), &supertypes()).expect("should succeed");

            assert_eq!(got.heartbeat_nid, NodeId::new(3, 1));
            let mailbox_nids = got
                .mailbox_refs
                .into_iter()
                .map(|r| r.node_id.node_id)
                .collect::<Vec<_>>();
            assert_eq!(mailbox_nids, [NodeId::new(3, 2), NodeId::new(3, 3)]);
        }

        #[test]
        fn heartbeat_missing() {
            let mut refs = children();
            refs.remove(0);

            let result = find_machine_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(TraceabilityInitializeError::MissingHeartbeat));
        }

        #[test]
        fn heartbeat_wrong_namespace() {
            let mut refs = children();
            refs[0] = node_ref(NodeClass::Variable, NS + 1, "Heartbeat", NodeId::new(3, 1));

            let result = find_machine_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(TraceabilityInitializeError::MissingHeartbeat));
        }

        #[test]
        fn mailboxes_missing() {
            let mut refs = children();
            refs.truncate(1);

            let result = find_machine_children(refs, NS, &config(), &supertypes());

            assert_matches!(result, Err(TraceabilityInitializeError::MissingMailboxes));
        }

        #[test]
        fn mailboxes_unknown_type() {
            let result = find_machine_children(children(), NS, &config(), &BrowseResults::new());

            assert_matches!(result, Err(TraceabilityInitializeError::MissingMailboxes));
        }

        #[test]
        fn mailbox_wrong_node_class() {
            let mut refs = children();
            refs[1] = ReferenceDescription {
                node_class: NodeClass::Variable,
                ..refs[1].clone()
            };

            let got =
                find_machine_children(refs, NS, &config(), &supertypes()).expect("should succeed");

            assert_eq!(got.mailbox_refs.len(), 1);
            assert_eq!(got.mailbox_refs[0].node_id.node_id, NodeId::new(3, 3));
        }
    }
}
