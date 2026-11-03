use std::collections::BTreeMap;
use std::sync::Arc;

use opcua::client::Session;
use opcua::types::NodeId;
use opcua_line_gateway_config::{AsciiDigitsOrUpper, NodeName};

use crate::TimeProvider;

use super::TraceabilityCache;

mod create_part_id;
mod handle_request;
mod read_part_sheet;
mod save_part_sheets;

/// Represents a traceability mailbox, containing request, response and part sheets.
pub(super) struct TraceabilityMailbox {
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
    /// Whether the request is the first one received.
    request_is_initial: bool,
    /// Raw part reference OPC-UA `NodeId`.
    raw_part_ref_node: NodeId,
    /// Raw material batch OPC-UA `NodeId`.
    raw_batch_node: NodeId,
    /// Part identifier OPC-UA `NodeId`.
    part_id_node: NodeId,
    /// General part sheet of this mailbox, as a mapping of `NodeName` to `NodeId`.
    general_part_sheet: BTreeMap<NodeName, NodeId>,
    /// `NodeName`s of the general part sheet nodes that are excluded from writing
    /// to the machine (i.e. when the machine asks to read the part sheet).
    excluded_read_nodes: Vec<NodeName>,
    /// Operation part sheet of this mailbox, as a mapping of `NodeName` to `NodeId`.
    operation_part_sheet: BTreeMap<NodeName, NodeId>,
}
