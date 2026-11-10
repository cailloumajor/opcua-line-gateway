use std::collections::BTreeMap;
use std::sync::Arc;

use opcua::client::Session;
use opcua::types::NodeId;
use opcua_line_gateway_config::{AsciiDigitsOrUpper, NodeName};

use crate::TimeProvider;

use super::TraceabilityCache;

pub(super) use initialize::MailboxInitializeError;

mod create_part_id;
mod handle_request;
mod initialize;
mod read_part_sheet;
mod save_part_sheets;

/// Initialized mailbox context.
pub(super) struct MailboxContext {
    /// Whether the request is the first one received.
    request_is_initial: bool,
    /// Request OPC-UA `NodeId`.
    request_nid: NodeId,
    /// Response OPC-UA `NOdeId`.
    response_nid: NodeId,
    /// Part identifier OPC-UA `NodeId`.
    part_id_nid: NodeId,
    /// Raw part reference OPC-UA `NodeId`.
    raw_part_ref_nid: NodeId,
    /// Raw material batch OPC-UA `NodeId`.
    raw_batch_nid: NodeId,
    /// General part sheet of this mailbox, as a mapping of `NodeName` to `NodeId`.
    general_part_sheet: BTreeMap<NodeName, NodeId>,
    /// `NodeName`s of the general part sheet nodes that are excluded from writing
    /// to the machine (i.e. when the machine asks to read the part sheet).
    excluded_read_nodes: Vec<NodeName>,
    /// Operation part sheet of this mailbox, as a mapping of `NodeName` to `NodeId`.
    operation_part_sheet: BTreeMap<NodeName, NodeId>,
}

/// Represents a traceability mailbox, containing request, response and part sheets.
pub(super) struct TraceabilityMailbox<S> {
    /// Production line identifier.
    line_id: AsciiDigitsOrUpper<2>,
    /// Machine identifier.
    machine_id: Arc<str>,
    /// Time provider.
    time_provider: TimeProvider,
    /// The traceability cache.
    cache: Arc<TraceabilityCache>,
    /// OPC-UA session.
    session: Arc<Session>,
    /// The state of this mailbox.
    state: S,
}

impl TraceabilityMailbox<()> {
    /// Create a new [`TraceabilityMailbox`], with an empty context.
    pub(super) fn new(
        line_id: AsciiDigitsOrUpper<2>,
        machine_id: Arc<str>,
        time_provider: TimeProvider,
        cache: Arc<TraceabilityCache>,
        session: Arc<Session>,
    ) -> Self {
        Self {
            line_id,
            machine_id,
            time_provider,
            cache,
            session,
            state: (),
        }
    }
}

impl TraceabilityMailbox<MailboxContext> {
    /// Get a reference to the request node of this mailbox.
    pub(super) fn request_node(&self) -> &NodeId {
        &self.state.request_nid
    }

    /// Get a reference to the response node of this maolbox.
    pub(super) fn response_node(&self) -> &NodeId {
        &self.state.response_nid
    }
}
