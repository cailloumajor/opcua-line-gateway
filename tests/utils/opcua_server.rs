//! OPC-UA test server, exposing a machine according to the traceability information
//! model described in `docs/information-model.md`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use opcua::crypto::{SecurityPolicy, X509};
use opcua::nodes::{
    ImportedItem, ImportedReference, NodeSetImport, ObjectBuilder, ObjectTypeBuilder,
    VariableBuilder,
};
use opcua::server::node_manager::memory::{SimpleNodeManager, simple_node_manager_imports};
use opcua::server::{
    ANONYMOUS_USER_TOKEN_ID, Limits, ServerBuilder, ServerEndpoint, ServerHandle, ServerUserToken,
    SubscriptionLimits,
};
use opcua::types::{
    AttributeId, DataEncoding, DataValue, MessageSecurityMode, NodeId, NodeSetNamespaceMapper,
    NumericRange, ObjectId, ObjectTypeId, QualifiedName, ReferenceTypeId, TimestampsToReturn,
    VariableTypeId, Variant,
};
use opcua_line_gateway_config::{
    AsciiDigitsOrUpper, Credential, OpcUaServerConfig, TraceabilityCommonOpcUaConfig,
    TraceabilityMachineOpcUaConfig,
};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{Instrument, info, info_span, instrument};

use super::gateway::gateway_pki_dir;

/// Common namespace URI.
const COMMON_NS_URI: &str = "urn:traceability:common";

/// BrowseNames (name part) of the nodes defined by the common namespace types.
pub mod common_name {
    pub const MACHINE_TYPE: &str = "TraceabilityMachineType";
    pub const MAILBOX_TYPE: &str = "TraceabilityMailboxType";
    pub const PART_SHEET_GENERAL_TYPE: &str = "TraceabilityPartSheetGeneralType";
    pub const PART_SHEET_OPERATION_TYPE: &str = "TraceabilityPartSheetOperationType";
    pub const HEARTBEAT: &str = "TraceabilityHeartbeat";
    pub const MAILBOX_PLACEHOLDER: &str = "<Mailbox>";
    pub const REQUEST: &str = "TraceabilityRequest";
    pub const RESPONSE: &str = "TraceabilityResponse";
    pub const PART_SHEET_GENERAL: &str = "TraceabilityPartSheetGeneral";
}

/// BrowseNames (name part) of some general part sheet variables, given a specific
/// meaning by the gateway configuration.
pub mod general {
    pub const PART_ID: &str = "NumeUniq";
    pub const RAW_PART_REF: &str = "RefePiecForg";
    pub const RAW_BATCH: &str = "LotMati";
    /// Variables excluded from the general part sheet written on load requests.
    pub const EXCLUDED_READ: [&str; 3] = ["QuiDeclNonConf", "QuiDeclConf", "ProgReceMach"];
}

/// Return the variables of the general part sheet, with their default values.
pub fn general_part_sheet_defaults() -> Vec<(&'static str, Variant)> {
    vec![
        (general::PART_ID, "".into()),
        (general::RAW_PART_REF, "".into()),
        (general::RAW_BATCH, "".into()),
        ("RefePiecFini", "".into()),
        ("PiecAnnoChgtRefPiecFini", false.into()),
        ("PiecAnnoChgtLotMati", false.into()),
        ("SuivTravPiecParPost", vec![false; 5].into()),
        ("SuivConfPiecParPost", vec![false; 5].into()),
        ("QuiDeclNonConf", "".into()),
        ("QuiDeclConf", "".into()),
        ("ProgReceMach", "".into()),
        ("SuivPostAvecPassTrav", vec![false; 5].into()),
        ("SuivPrelPiecParPost", vec![false; 5].into()),
        ("SuivRejePiecParPost", vec![false; 5].into()),
        ("RefePiecFiniInco", false.into()),
        ("PiecTravInco", false.into()),
        ("PiecNonConfAmon", false.into()),
        ("LotMatiInco", false.into()),
        ("PiecRebu", false.into()),
        ("DeclRebuMoti", "".into()),
        ("DeclRebuQui", "".into()),
    ]
}

/// Numeric node identifiers of the common namespace, fixed by its nodeset.
mod common_nid {
    pub(super) const PART_SHEET_GENERAL_TYPE: u32 = 1;
    pub(super) const PART_SHEET_OPERATION_TYPE: u32 = 2;
    pub(super) const MAILBOX_TYPE: u32 = 3;
    pub(super) const MACHINE_TYPE: u32 = 4;
    pub(super) const MAILBOX_TYPE_REQUEST: u32 = 10;
    pub(super) const MAILBOX_TYPE_RESPONSE: u32 = 11;
    pub(super) const MAILBOX_TYPE_PART_SHEET_GENERAL: u32 = 12;
    pub(super) const MACHINE_TYPE_HEARTBEAT: u32 = 20;
    pub(super) const MACHINE_TYPE_MAILBOX: u32 = 21;
    /// First identifier of the variables of the general part sheet type.
    pub(super) const PART_SHEET_GENERAL_TYPE_VARIABLES: u32 = 100;
}

/// Build and return traceability common OPC-UA configuration.
///
/// Nodes are identified by their BrowseNames, defined by the common namespace types,
/// rather than by node identifiers.
pub fn traceability_common_opcua_config() -> TraceabilityCommonOpcUaConfig {
    TraceabilityCommonOpcUaConfig {
        namespace_url: COMMON_NS_URI.into(),
        part_id_name: general::PART_ID.into(),
        raw_part_ref_name: general::RAW_PART_REF.into(),
        raw_batch_name: general::RAW_BATCH.into(),
        excluded_read_names: general::EXCLUDED_READ.map(Into::into).to_vec(),
    }
}

/// Description of a machine of the production line, exposed by a test server
/// through its operation namespace.
#[derive(Clone)]
pub struct TestMachine {
    /// Name of the operation, e.g. `OP30`, which is also the BrowseName of the
    /// machine object.
    operation: &'static str,
    /// BrowseNames of the mailboxes of the machine.
    mailboxes: Vec<&'static str>,
    /// Variables of the operation part sheet, with their initial values.
    operation_variables: Vec<(&'static str, Variant)>,
}

impl TestMachine {
    /// Create a machine performing the given operation, with the given mailboxes.
    pub fn new(
        operation: &'static str,
        mailboxes: &[&'static str],
        operation_variables: Vec<(&'static str, Variant)>,
    ) -> Self {
        assert!(!mailboxes.is_empty(), "a machine has at least one mailbox");
        Self {
            operation,
            mailboxes: mailboxes.to_vec(),
            operation_variables,
        }
    }

    /// Return the URI of the operation namespace.
    pub fn namespace_uri(&self) -> String {
        format!("urn:traceability:{}", self.operation)
    }
}

/// Numeric node identifiers of the nodes of a mailbox instance, in the operation
/// namespace.
#[derive(Clone)]
struct MailboxNids {
    request: u32,
    response: u32,
    general: BTreeMap<&'static str, u32>,
    operation: BTreeMap<&'static str, u32>,
}

/// Numeric node identifiers of the nodes of a machine instance, in the operation
/// namespace.
#[derive(Clone)]
struct MachineNids {
    machine: u32,
    heartbeat: u32,
    mailboxes: BTreeMap<&'static str, MailboxNids>,
}

/// Builder of the nodes to import, with references to their parent and type.
struct NodesBuilder {
    items: Vec<ImportedItem>,
}

impl NodesBuilder {
    /// Add an object type, subtype of the given type.
    fn object_type(
        &mut self,
        node_id: NodeId,
        browse_name: QualifiedName,
        supertype: NodeId,
        is_abstract: bool,
    ) {
        let display_name = browse_name.name.to_string();
        self.items.push(ImportedItem {
            node: ObjectTypeBuilder::new(&node_id, browse_name, display_name)
                .is_abstract(is_abstract)
                .build()
                .into(),
            references: vec![ImportedReference {
                target_id: supertype,
                type_id: ReferenceTypeId::HasSubtype.into(),
                is_forward: false,
            }],
        });
    }

    /// Add an object, component of the given parent (or organized in the `Objects`
    /// folder if there is none).
    fn object(
        &mut self,
        node_id: NodeId,
        browse_name: QualifiedName,
        type_definition: NodeId,
        parent: Option<&NodeId>,
        modelling_rule: Option<ObjectId>,
    ) {
        let display_name = browse_name.name.to_string();
        let mut references = vec![
            Self::parent_reference(parent),
            ImportedReference {
                target_id: type_definition,
                type_id: ReferenceTypeId::HasTypeDefinition.into(),
                is_forward: true,
            },
        ];
        references.extend(Self::modelling_rule_reference(modelling_rule));
        self.items.push(ImportedItem {
            node: ObjectBuilder::new(&node_id, browse_name, display_name)
                .build()
                .into(),
            references,
        });
    }

    /// Add a data variable, component of the given parent.
    fn variable(
        &mut self,
        node_id: NodeId,
        browse_name: QualifiedName,
        value: Variant,
        writable: bool,
        parent: &NodeId,
        modelling_rule: Option<ObjectId>,
    ) {
        let display_name = browse_name.name.to_string();
        let mut builder = VariableBuilder::new(&node_id, browse_name, display_name)
            .data_type(value.data_type().unwrap().node_id)
            .value(value);
        if writable {
            builder = builder.writable();
        }
        let mut references = vec![
            Self::parent_reference(Some(parent)),
            ImportedReference {
                target_id: VariableTypeId::BaseDataVariableType.into(),
                type_id: ReferenceTypeId::HasTypeDefinition.into(),
                is_forward: true,
            },
        ];
        references.extend(Self::modelling_rule_reference(modelling_rule));
        self.items.push(ImportedItem {
            node: builder.build().into(),
            references,
        });
    }

    fn parent_reference(parent: Option<&NodeId>) -> ImportedReference {
        match parent {
            Some(parent) => ImportedReference {
                target_id: parent.clone(),
                type_id: ReferenceTypeId::HasComponent.into(),
                is_forward: false,
            },
            None => ImportedReference {
                target_id: ObjectId::ObjectsFolder.into(),
                type_id: ReferenceTypeId::Organizes.into(),
                is_forward: false,
            },
        }
    }

    fn modelling_rule_reference(modelling_rule: Option<ObjectId>) -> Option<ImportedReference> {
        modelling_rule.map(|rule| ImportedReference {
            target_id: rule.into(),
            type_id: ReferenceTypeId::HasModellingRule.into(),
            is_forward: true,
        })
    }
}

/// Add the types of the common namespace, which has no instances.
fn build_common_namespace(nodes: &mut NodesBuilder, ns: u16) {
    use common_name as name;

    let nid = |id| NodeId::new(ns, id);
    let qn = |name: &str| QualifiedName::new(ns, name);
    let mandatory = Some(ObjectId::ModellingRule_Mandatory);

    // General part sheet type, with its variables.
    let general_type = nid(common_nid::PART_SHEET_GENERAL_TYPE);
    nodes.object_type(
        general_type.clone(),
        qn(name::PART_SHEET_GENERAL_TYPE),
        ObjectTypeId::BaseObjectType.into(),
        false,
    );
    for (id, (var_name, value)) in
        (common_nid::PART_SHEET_GENERAL_TYPE_VARIABLES..).zip(general_part_sheet_defaults())
    {
        nodes.variable(nid(id), qn(var_name), value, true, &general_type, mandatory);
    }

    // Abstract operation part sheet type, without variables.
    nodes.object_type(
        nid(common_nid::PART_SHEET_OPERATION_TYPE),
        qn(name::PART_SHEET_OPERATION_TYPE),
        ObjectTypeId::BaseObjectType.into(),
        true,
    );

    // Mailbox type.
    let mailbox_type = nid(common_nid::MAILBOX_TYPE);
    nodes.object_type(
        mailbox_type.clone(),
        qn(name::MAILBOX_TYPE),
        ObjectTypeId::BaseObjectType.into(),
        false,
    );
    nodes.variable(
        nid(common_nid::MAILBOX_TYPE_REQUEST),
        qn(name::REQUEST),
        0u8.into(),
        false,
        &mailbox_type,
        mandatory,
    );
    nodes.variable(
        nid(common_nid::MAILBOX_TYPE_RESPONSE),
        qn(name::RESPONSE),
        0u8.into(),
        true,
        &mailbox_type,
        mandatory,
    );
    nodes.object(
        nid(common_nid::MAILBOX_TYPE_PART_SHEET_GENERAL),
        qn(name::PART_SHEET_GENERAL),
        general_type,
        Some(&mailbox_type),
        mandatory,
    );

    // Machine type.
    let machine_type = nid(common_nid::MACHINE_TYPE);
    nodes.object_type(
        machine_type.clone(),
        qn(name::MACHINE_TYPE),
        ObjectTypeId::BaseObjectType.into(),
        false,
    );
    nodes.variable(
        nid(common_nid::MACHINE_TYPE_HEARTBEAT),
        qn(name::HEARTBEAT),
        false.into(),
        true,
        &machine_type,
        mandatory,
    );
    nodes.object(
        nid(common_nid::MACHINE_TYPE_MAILBOX),
        qn(name::MAILBOX_PLACEHOLDER),
        mailbox_type,
        Some(&machine_type),
        Some(ObjectId::ModellingRule_MandatoryPlaceholder),
    );
}

/// Add the types and the machine instance of an operation namespace, returning the
/// numeric identifiers of the instance nodes.
///
/// Identifiers are allocated sequentially, so that the nodes of each mailbox have
/// distinct identifiers, as in the nodesets of the controllers. The allocation does
/// not depend on the namespace indexes.
fn build_operation_namespace(
    nodes: &mut NodesBuilder,
    machine: &TestMachine,
    common_ns: u16,
    operation_ns: u16,
) -> MachineNids {
    use common_name as name;

    let mut next_id = 1u32..;
    let mut alloc = move || next_id.next().unwrap();
    let common = |id| NodeId::new(common_ns, id);
    let common_qn = |name: &str| QualifiedName::new(common_ns, name);
    let nid = |id| NodeId::new(operation_ns, id);
    let qn = |name: &str| QualifiedName::new(operation_ns, name);
    let mandatory = Some(ObjectId::ModellingRule_Mandatory);
    let op = machine.operation;

    // Operation part sheet type.
    let part_sheet_type = nid(alloc());
    nodes.object_type(
        part_sheet_type.clone(),
        qn(&format!("TraceabilityPartSheet{op}Type")),
        common(common_nid::PART_SHEET_OPERATION_TYPE),
        false,
    );
    for (var_name, value) in machine.operation_variables.clone() {
        nodes.variable(
            nid(alloc()),
            qn(var_name),
            value,
            true,
            &part_sheet_type,
            mandatory,
        );
    }

    // Mailbox type, adding the operation part sheet to the common mailbox type.
    let part_sheet_name = format!("TraceabilityPartSheet{op}");
    let mailbox_type = nid(alloc());
    nodes.object_type(
        mailbox_type.clone(),
        qn(&format!("TraceabilityMailbox{op}Type")),
        common(common_nid::MAILBOX_TYPE),
        false,
    );
    nodes.object(
        nid(alloc()),
        qn(&part_sheet_name),
        part_sheet_type.clone(),
        Some(&mailbox_type),
        mandatory,
    );

    // Machine instance, fully expanded.
    let machine_id = alloc();
    let machine_node = nid(machine_id);
    nodes.object(
        machine_node.clone(),
        qn(op),
        common(common_nid::MACHINE_TYPE),
        None,
        None,
    );
    let heartbeat = alloc();
    nodes.variable(
        nid(heartbeat),
        common_qn(name::HEARTBEAT),
        false.into(),
        true,
        &machine_node,
        None,
    );

    // A component of the machine object which is not a mailbox, as its type does
    // not derive from the mailbox type, despite its name.
    nodes.object(
        nid(alloc()),
        qn(&format!("TraceabilityMailbox{op}Spare")),
        ObjectTypeId::BaseObjectType.into(),
        Some(&machine_node),
        None,
    );

    let mut mailboxes = BTreeMap::new();
    for &mailbox_name in &machine.mailboxes {
        let mailbox = nid(alloc());
        nodes.object(
            mailbox.clone(),
            qn(mailbox_name),
            mailbox_type.clone(),
            Some(&machine_node),
            None,
        );

        let request = alloc();
        nodes.variable(
            nid(request),
            common_qn(name::REQUEST),
            0u8.into(),
            false,
            &mailbox,
            None,
        );
        let response = alloc();
        nodes.variable(
            nid(response),
            common_qn(name::RESPONSE),
            0u8.into(),
            true,
            &mailbox,
            None,
        );

        let general_part_sheet = nid(alloc());
        nodes.object(
            general_part_sheet.clone(),
            common_qn(name::PART_SHEET_GENERAL),
            common(common_nid::PART_SHEET_GENERAL_TYPE),
            Some(&mailbox),
            None,
        );
        let mut general = BTreeMap::new();
        for (var_name, value) in general_part_sheet_defaults() {
            let id = alloc();
            nodes.variable(
                nid(id),
                common_qn(var_name),
                value,
                true,
                &general_part_sheet,
                None,
            );
            general.insert(var_name, id);
        }

        let operation_part_sheet = nid(alloc());
        nodes.object(
            operation_part_sheet.clone(),
            qn(&part_sheet_name),
            part_sheet_type.clone(),
            Some(&mailbox),
            None,
        );
        let mut operation = BTreeMap::new();
        for (var_name, value) in machine.operation_variables.clone() {
            let id = alloc();
            nodes.variable(
                nid(id),
                qn(var_name),
                value,
                true,
                &operation_part_sheet,
                None,
            );
            operation.insert(var_name, id);
        }

        mailboxes.insert(
            mailbox_name,
            MailboxNids {
                request,
                response,
                general,
                operation,
            },
        );
    }

    MachineNids {
        machine: machine_id,
        heartbeat,
        mailboxes,
    }
}

/// Nodeset importer for testing this application: the common namespace, and the
/// operation namespace of a machine.
struct TestingNodes {
    namespaces: [String; 2],
    machine: TestMachine,
}

impl NodeSetImport for TestingNodes {
    fn register_namespaces(&self, mapper: &mut NodeSetNamespaceMapper) {
        for (index, namespace) in (1..).zip(&self.namespaces) {
            mapper.add_namespace(namespace, index);
        }
    }

    fn get_own_namespaces(&self) -> Vec<String> {
        self.namespaces.to_vec()
    }

    fn load<'a>(
        &'a self,
        namespaces: &'a NodeSetNamespaceMapper,
    ) -> Box<dyn Iterator<Item = ImportedItem> + 'a> {
        let common_ns = namespaces
            .get_index(1)
            .expect("getting the common namespace index should not fail");
        let operation_ns = namespaces
            .get_index(2)
            .expect("getting the operation namespace index should not fail");

        let mut nodes = NodesBuilder { items: Vec::new() };
        build_common_namespace(&mut nodes, common_ns);
        build_operation_namespace(&mut nodes, &self.machine, common_ns, operation_ns);

        Box::new(nodes.items.into_iter())
    }
}

/// Represents an OPC-UA server to be used in tests, exposing a single machine.
pub struct OpcUaTestServer {
    name: String,
    url: String,
    secured: bool,
    line_id: Option<AsciiDigitsOrUpper<2>>,
    namespace_uri: String,
    nids: MachineNids,
    node_manager: Arc<SimpleNodeManager>,
    operation_ns_index: u16,
    server_handle: ServerHandle,
    task_handle: JoinHandle<Result<(), String>>,
}

impl OpcUaTestServer {
    /// Start and return an [`OpcUaTestServer`] exposing the given machine.
    pub async fn start(
        secured: bool,
        temp_dir: &Path,
        line_id: Option<AsciiDigitsOrUpper<2>>,
        machine: TestMachine,
    ) -> Self {
        // Bind to localhost IPv4 address and a free port.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("TCP binding should not fail");
        let local_addr = listener
            .local_addr()
            .expect("getting the listener local address should not fail");
        let name = format!("test_server_{}", local_addr.port());
        let url = format!("opc.tcp://{local_addr}/");

        let pki_dir = temp_dir.join(format!("server_pki_{}", local_addr.port()));

        // The identifiers do not depend on the namespace indexes, which are only
        // known once the nodes are imported.
        let namespace_uri = machine.namespace_uri();
        let nids =
            build_operation_namespace(&mut NodesBuilder { items: Vec::new() }, &machine, 0, 0);

        let mut builder = ServerBuilder::new()
            .application_name("OPC-UA testing server")
            .application_uri(format!("urn:test-server:{}", local_addr.port()))
            .product_uri(format!("urn:{}:test-server", env!("CARGO_PKG_NAME")))
            .pki_dir(&pki_dir)
            .create_sample_keypair(true)
            .host(local_addr.ip().to_string())
            .port(local_addr.port())
            .discovery_urls(vec![url.clone()])
            .subscription_poll_interval_ms(10)
            .limits(Limits {
                subscriptions: SubscriptionLimits {
                    min_publishing_interval_ms: 10.0,
                    // Work around an async-opcua 0.19 server bug, where a value notified
                    // before the end of the sampling interval is held back until the next
                    // notification, by disabling sampling interval filtering.
                    min_sampling_interval_ms: 0.0,
                    ..Default::default()
                },
                ..Default::default()
            })
            .with_node_manager(simple_node_manager_imports(
                vec![Box::new(TestingNodes {
                    namespaces: [COMMON_NS_URI.into(), namespace_uri.clone()],
                    machine,
                })],
                "testing-nodes-manager",
            ));
        if secured {
            builder = builder
                .add_user_token(
                    "test_password_user",
                    ServerUserToken {
                        user: "testuser".to_owned(),
                        pass: Some("testpassword".to_owned()),
                        ..Default::default()
                    },
                )
                .add_endpoint(
                    "Aes256Sha256RsaPss - Sign & Encrypt",
                    ServerEndpoint::new_aes256_sha256_rsapss_sign_encrypt(
                        "/",
                        &["test_password_user".into()],
                    ),
                )
        } else {
            builder = builder.add_endpoint(
                "None",
                ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.into()]),
            )
        }
        let (server, server_handle) = builder
            .build()
            .expect("building the server should not fail");

        let node_manager = server_handle
            .node_managers()
            .get_of_type::<SimpleNodeManager>()
            .expect("node manager should exist");

        let operation_ns_index = server_handle
            .get_namespace_index(&namespace_uri)
            .expect("operation namespace index should exist");

        let task_handle = tokio::spawn(
            server
                .run_with(listener)
                .instrument(info_span!("testing server", %local_addr)),
        );

        // Wait for server certificate to have been provisioned.
        let my_cert_file = pki_dir.join("own/cert.der");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if std::fs::metadata(&my_cert_file).is_ok() {
                    return;
                }
            }
        })
        .await
        .expect("waiting for server cert and key should not time out");

        // Make certificates copies to make gateway and server mutually trust.
        let gateway_pki_dir = gateway_pki_dir(temp_dir);
        copy_cert_to_trusted(&my_cert_file, &gateway_pki_dir);
        let gateway_cert_path = gateway_pki_dir.join("own/opcua-line-gateway-cert.der");
        copy_cert_to_trusted(&gateway_cert_path, &pki_dir);

        Self {
            name,
            url,
            secured,
            line_id,
            namespace_uri,
            nids,
            node_manager,
            operation_ns_index,
            server_handle,
            task_handle,
        }
    }

    /// Return the server name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the line identifier that was passed when starting this server.
    pub fn line_id(&self) -> Option<AsciiDigitsOrUpper<2>> {
        self.line_id
    }

    /// Return the OPC-UA configuration for this server.
    pub fn opcua_config(&self) -> OpcUaServerConfig {
        let (security_policy, security_mode) = if self.secured {
            (
                SecurityPolicy::Aes256Sha256RsaPss,
                MessageSecurityMode::SignAndEncrypt,
            )
        } else {
            (SecurityPolicy::None, MessageSecurityMode::None)
        };

        OpcUaServerConfig {
            url: self.url.clone(),
            security_policy,
            security_mode,
        }
    }

    /// Return the traceability OPC-UA configuration of the machine: its operation
    /// namespace and its machine object, from which all its nodes are discovered.
    pub fn traceability_machine_opcua_config(&self) -> TraceabilityMachineOpcUaConfig {
        TraceabilityMachineOpcUaConfig {
            namespace_url: self.namespace_uri.clone(),
            machine_nid: self.nids.machine,
        }
    }

    /// Return the credentials to use for authenticating with this server.
    pub fn opcua_credentials(&self) -> Option<Credential> {
        self.secured.then_some(Credential {
            user: "testuser".into(),
            password: "testpassword".into(),
        })
    }

    /// Return a handle on the mailbox with the given BrowseName.
    pub fn mailbox(&self, name: &str) -> TestMailbox<'_> {
        let nids = self
            .nids
            .mailboxes
            .get(name)
            .unwrap_or_else(|| panic!("mailbox {name} should exist"));
        TestMailbox {
            server: self,
            name: name.to_owned(),
            nids,
        }
    }

    /// Return handles on all the mailboxes of the machine.
    pub fn mailboxes(&self) -> Vec<TestMailbox<'_>> {
        self.nids
            .mailboxes
            .keys()
            .map(|name| self.mailbox(name))
            .collect()
    }

    /// Read the value of the heartbeat of the machine.
    pub fn read_heartbeat(&self) -> Variant {
        self.read_node(self.nids.heartbeat)
    }

    /// Wait for a client to have subscribed to the request code value of every mailbox.
    #[instrument(skip_all, fields(url = self.url))]
    pub async fn wait_request_subscriptions(&self) {
        info!("waiting for a client to subscribe to the request codes");

        for mailbox in self.mailboxes() {
            let node_id = self.node_id(mailbox.nids.request);
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    // The sampling closure is only called for enabled monitored items,
                    // returning `None` so that no notification is actually sent.
                    let subscribed = Cell::new(false);
                    self.server_handle.subscriptions().maybe_notify(
                        [(&node_id, AttributeId::Value)].into_iter(),
                        |_, _, _, _| {
                            subscribed.set(true);
                            None
                        },
                    );
                    if subscribed.get() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "waiting for request code subscription of mailbox {} should not time out",
                    mailbox.name
                )
            });
        }
    }

    fn node_id(&self, nid: u32) -> NodeId {
        NodeId::new(self.operation_ns_index, nid)
    }

    /// Write the value of a variable of the operation namespace.
    fn write_node(&self, nid: u32, value: Variant) {
        self.node_manager
            .set_value(
                self.server_handle.subscriptions(),
                &self.node_id(nid),
                None,
                DataValue::new_now(value),
            )
            .expect("writing a value in the operation namespace should not fail");
    }

    /// Read the value of a variable of the operation namespace.
    fn read_node(&self, nid: u32) -> Variant {
        let address_space = self.node_manager.address_space().read();
        let node = address_space
            .find_node(&self.node_id(nid))
            .expect("node should exist in address space");
        let data_value = node
            .as_node()
            .get_attribute(
                TimestampsToReturn::Neither,
                AttributeId::Value,
                &NumericRange::None,
                &DataEncoding::Binary,
            )
            .expect("getting node value should not fail");
        data_value.value.expect("node should have a value")
    }

    /// Stop the server and wait for it to have finished its work.
    pub async fn stop(self) {
        self.server_handle.cancel();
        tokio::time::timeout(Duration::from_secs(10), self.task_handle)
            .await
            .expect("waiting for server to finish should not time out")
            .expect("joining the server task should not fail")
            .expect("server task should not have failed");
    }
}

/// Handle on a mailbox of the machine exposed by an [`OpcUaTestServer`], acting as
/// the machine program.
pub struct TestMailbox<'a> {
    server: &'a OpcUaTestServer,
    name: String,
    nids: &'a MailboxNids,
}

impl TestMailbox<'_> {
    /// Write the request code.
    pub fn write_request(&self, code: u8) {
        self.server.write_node(self.nids.request, code.into());
    }

    /// Write the response code, as if it had been written by the gateway.
    pub fn write_response(&self, code: u8) {
        self.server.write_node(self.nids.response, code.into());
    }

    /// Read the response code.
    pub fn read_response(&self) -> u8 {
        match self.server.read_node(self.nids.response) {
            Variant::Byte(code) => code,
            other => panic!(
                "response of mailbox {} should be a byte: {other:?}",
                self.name
            ),
        }
    }

    /// Write a variable of the general part sheet, given its BrowseName.
    pub fn write_general(&self, name: &str, value: impl Into<Variant>) {
        self.server.write_node(self.general_nid(name), value.into());
    }

    /// Read a variable of the general part sheet, given its BrowseName.
    pub fn read_general(&self, name: &str) -> Variant {
        self.server.read_node(self.general_nid(name))
    }

    /// Write a variable of the operation part sheet, given its BrowseName.
    pub fn write_operation(&self, name: &str, value: impl Into<Variant>) {
        let nid = *self
            .nids
            .operation
            .get(name)
            .unwrap_or_else(|| panic!("operation variable {name} should exist"));
        self.server.write_node(nid, value.into());
    }

    fn general_nid(&self, name: &str) -> u32 {
        *self
            .nids
            .general
            .get(name)
            .unwrap_or_else(|| panic!("general part sheet variable {name} should exist"))
    }

    /// Wait for the response code to match the given predicate, and return it.
    pub async fn wait_response(&self, predicate: impl Fn(u8) -> bool) -> u8 {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let response = self.read_response();
                if predicate(response) {
                    return response;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "waiting for response of mailbox {} should not time out",
                self.name
            )
        })
    }

    /// Reset the request code and wait for the response reset.
    pub async fn traceability_reset(&self) {
        self.write_request(0);
        self.wait_response(|code| code == 0).await;
    }

    /// Run a full request cycle: write the request code, wait for a non-zero response,
    /// then reset the request and wait for the response reset. Returns the response code.
    pub async fn traceability_request(&self, code: u8) -> u8 {
        self.write_request(code);
        let response = self.wait_response(|code| code != 0).await;
        self.traceability_reset().await;

        response
    }
}

fn copy_cert_to_trusted(cert_path: &Path, target_pki: &Path) {
    let raw_cert = std::fs::read(cert_path).expect("reading the certificate should not fail");
    let x509_cert = X509::from_der(&raw_cert).expect("loading certificate as X509 should not fail");
    let common_name = x509_cert
        .common_name()
        .expect("getting certificate common name should not fail");
    let digest = x509_cert.thumbprint().as_hex_string();
    let trusted_cert_name = format!("{common_name} [{digest}].der");
    std::fs::copy(
        cert_path,
        target_pki.join("trusted").join(trusted_cert_name),
    )
    .expect("copying the certificate to trusted directory should not fail");
}
