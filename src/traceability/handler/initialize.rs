use std::sync::Arc;

use opcua::types::{NodeClass, NodeId, QualifiedName};
use opcua_line_gateway_config::{TraceabilityCommonConfig, TraceabilityMachineConfig};
use thiserror::Error;
use tracing::{debug, instrument};

use crate::traceability::mailbox::{MailboxInitializeError, TraceabilityMailbox};

use super::{BrowseMachineError, HandlerContext, TraceabilityHandler};

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
    #[error("error initializing mailbox")]
    MailboxInitialize(#[source] MailboxInitializeError),
}

impl TraceabilityHandler<()> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
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

        // Whether a `BrowseName` is the given name, in the common namespace.
        let is_common_name = |browse_name: &QualifiedName, name: &str| {
            browse_name.namespace_index == common_ns_index && browse_name.name == name
        };

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

        // Extract heartbeat and  mailboxes nodes.
        let mut heartbeat = None;
        let mut mailboxes = Vec::new();
        for node_ref in machine_children {
            let browse_name = &node_ref.browse_name;
            let is_mailbox = || {
                supertypes
                    .get(&node_ref.type_definition.node_id)
                    .is_some_and(|refs| {
                        refs.iter().any(|r| {
                            is_common_name(&r.browse_name, &common_config.opc_ua.mailbox_type)
                        })
                    })
            };

            match node_ref.node_class {
                NodeClass::Variable
                    if is_common_name(browse_name, &common_config.opc_ua.heartbeat_node) =>
                {
                    heartbeat = Some(node_ref.node_id.node_id);
                }
                NodeClass::Object if is_mailbox() => {
                    let mailbox = TraceabilityMailbox::new(
                        common_config.line_id,
                        Arc::clone(&self.machine_id),
                        self.time_provider.clone(),
                        Arc::clone(&self.cache),
                        Arc::clone(&self.session),
                    );
                    let mailbox = mailbox
                        .initialize(
                            common_ns_index,
                            &common_config.opc_ua,
                            &mut children,
                            &supertypes,
                            node_ref,
                        )
                        .await
                        .map_err(TraceabilityInitializeError::MailboxInitialize)?;

                    mailboxes.push(mailbox);
                }
                _ => {
                    debug!(msg = "ignored machine node child", %browse_name);
                }
            }
        }

        let heartbeat = heartbeat.ok_or(TraceabilityInitializeError::MissingHeartbeat)?;

        let state = HandlerContext {
            heartbeat,
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
