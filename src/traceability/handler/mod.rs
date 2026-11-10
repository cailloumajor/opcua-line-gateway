use std::collections::HashMap;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt;
use opcua::client::browser::{BrowseResultItem, Browser};
use opcua::client::{DefaultRetryPolicy, ExponentialBackoff, Session};
use opcua::types::{
    BrowseDescription, BrowseDirection, BrowseResultMaskFlags, NodeClass, NodeClassMask, NodeId,
    ReferenceDescription, ReferenceTypeId, StatusCode,
};
use thiserror::Error;
use tracing::instrument;

use crate::time::TimeProvider;

use super::cache::TraceabilityCache;
use super::mailbox::{MailboxContext, TraceabilityMailbox};

pub(crate) use initialize::TraceabilityInitializeError;
pub(crate) use install::TraceabilityInstallError;

mod initialize;
mod install;

/// Aggregation of browsed nodes.
pub(super) type BrowseResults = HashMap<NodeId, Vec<ReferenceDescription>>;

/// Whether the type `type_definition` has a direct supertype with the given
/// `BrowseName` in the common namespace, according to the browsed `supertypes`.
pub(super) fn has_common_supertype(
    supertypes: &BrowseResults,
    type_definition: &NodeId,
    common_ns_index: u16,
    name: &str,
) -> bool {
    supertypes.get(type_definition).is_some_and(|refs| {
        refs.iter()
            .any(|r| r.browse_name.namespace_index == common_ns_index && r.browse_name.name == name)
    })
}

/// Errors that can occur during browsing of the root machine object.
#[derive(Debug, Error)]
pub(crate) enum BrowseMachineError {
    #[error("browse request error: {0}")]
    Service(#[source] opcua::types::Error),
    #[error("bad BrowseResult status code for node {0}: {1}")]
    BrowseResultStatus(NodeId, StatusCode),
}

/// The traceability handler state after initialization.
pub(crate) struct HandlerContext {
    /// [`NodeId`] of the heartbeat variable.
    heartbeat: NodeId,
    /// Initialized traceability mailboxes.
    mailboxes: Vec<TraceabilityMailbox<MailboxContext>>,
}

/// Manages traceability for an OPC-UA session.
pub(crate) struct TraceabilityHandler<S> {
    /// The ID of the machine this handler works with.
    machine_id: Arc<str>,
    /// The OPC-UA session.
    session: Arc<Session>,
    /// The traceability cache.
    cache: Arc<TraceabilityCache>,
    /// Time provider.
    time_provider: TimeProvider,
    /// The state of this handler.
    state: S,
}

impl TraceabilityHandler<()> {
    /// Create a new [`TraceabilityHandler`].
    pub(crate) fn new(
        machine_id: Arc<str>,
        session: Arc<Session>,
        cache: Arc<TraceabilityCache>,
        time_provider: TimeProvider,
    ) -> Self {
        Self {
            machine_id,
            session,
            cache,
            time_provider,
            state: (),
        }
    }

    /// Browse the machine root object recursively to discover all of its object
    /// and variable children.
    ///
    /// Return two mappings of references per node, respectively children one and
    /// supertype one.
    #[instrument(err, skip(self))]
    async fn browse_machine(
        &self,
        machine_node: NodeId,
    ) -> Result<(BrowseResults, BrowseResults), BrowseMachineError> {
        // Prepare the browser configuration.
        let retry_policy = DefaultRetryPolicy::new(ExponentialBackoff::new(
            Duration::from_secs(5),     // max sleep
            Some(3),                    // max retries
            Duration::from_millis(500), // initial sleep
        ));
        let browse_policy = |item: &BrowseResultItem| -> Vec<BrowseDescription> {
            // Supertype of a type: nothing more to browse.
            if item.request().browse_direction == BrowseDirection::Inverse {
                return Vec::new();
            }
            item.references()
                .iter()
                .filter(|r| r.node_class == NodeClass::Object && r.node_id.server_index == 0)
                .flat_map(|r| {
                    [
                        components_of(r.node_id.node_id.clone()),
                        supertype_of(r.type_definition.node_id.clone()),
                    ]
                })
                .collect()
        };

        let mut children: HashMap<NodeId, Vec<ReferenceDescription>> = HashMap::new();
        let mut supertypes: HashMap<NodeId, Vec<ReferenceDescription>> = HashMap::new();

        // Build the browser and collect the results.
        let browser = Browser::new(&self.session, browse_policy, retry_policy);
        let mut pinned_stream = pin!(browser.run(vec![components_of(machine_node)]));
        while let Some(item) = pinned_stream
            .try_next()
            .await
            .map_err(BrowseMachineError::Service)?
        {
            let status = item.status();
            if !status.is_good() {
                return Err(BrowseMachineError::BrowseResultStatus(
                    item.parent_id().clone(),
                    status,
                ));
            }

            let is_inverse = item.request().browse_direction == BrowseDirection::Inverse;
            let (parent_id, references) = item.into_results();
            if is_inverse {
                supertypes.entry(parent_id).or_default().extend(references);
            } else {
                children.entry(parent_id).or_default().extend(references);
            }
        }

        Ok((children, supertypes))
    }
}

/// Build the [`NodeDescription`] to browse the components (objects and variables)
/// of a node.
fn components_of(node_id: NodeId) -> BrowseDescription {
    let node_class_mask = NodeClassMask::OBJECT | NodeClassMask::VARIABLE;
    let result_mask = BrowseResultMaskFlags::NodeClass
        | BrowseResultMaskFlags::BrowseName
        | BrowseResultMaskFlags::TypeDefinition;

    BrowseDescription {
        node_id,
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HasComponent.into(),
        include_subtypes: true,
        node_class_mask: node_class_mask.bits(),
        // BrowseName, NodeClass and TypeDefinition are needed.
        result_mask: result_mask.bits(),
    }
}

/// Build the [`NodeDescription`] to browse the direct supertype of a type.
fn supertype_of(type_id: NodeId) -> BrowseDescription {
    BrowseDescription {
        node_id: type_id,
        browse_direction: BrowseDirection::Inverse,
        reference_type_id: ReferenceTypeId::HasSubtype.into(),
        include_subtypes: false,
        node_class_mask: NodeClassMask::OBJECT_TYPE.bits(),
        result_mask: BrowseResultMaskFlags::BrowseName.bits(),
    }
}

#[cfg(test)]
mod tests {
    use opcua::types::QualifiedName;

    use super::*;

    mod has_common_supertype {
        use super::*;

        const NS: u16 = 2;

        fn supertypes() -> BrowseResults {
            BrowseResults::from([(
                NodeId::new(3, 1000),
                vec![ReferenceDescription {
                    browse_name: QualifiedName::new(NS, "SuperType"),
                    node_class: NodeClass::ObjectType,
                    ..Default::default()
                }],
            )])
        }

        #[test]
        fn found() {
            assert!(has_common_supertype(
                &supertypes(),
                &NodeId::new(3, 1000),
                NS,
                "SuperType"
            ));
        }

        #[test]
        fn other_name() {
            assert!(!has_common_supertype(
                &supertypes(),
                &NodeId::new(3, 1000),
                NS,
                "OtherType"
            ));
        }

        #[test]
        fn other_namespace() {
            assert!(!has_common_supertype(
                &supertypes(),
                &NodeId::new(3, 1000),
                NS + 1,
                "SuperType"
            ));
        }

        #[test]
        fn unknown_type() {
            assert!(!has_common_supertype(
                &supertypes(),
                &NodeId::new(3, 1001),
                NS,
                "SuperType"
            ));
        }
    }
}
