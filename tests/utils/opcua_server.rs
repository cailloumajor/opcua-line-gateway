use std::cell::Cell;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use opcua::crypto::SecurityPolicy;
use opcua::nodes::{
    ImportedItem, ImportedReference, NodeSetImport, ObjectBuilder, VariableBuilder,
};
use opcua::server::node_manager::memory::{SimpleNodeManager, simple_node_manager_imports};
use opcua::server::{
    ANONYMOUS_USER_TOKEN_ID, Limits, ServerBuilder, ServerEndpoint, ServerHandle, ServerUserToken,
    SubscriptionLimits,
};
use opcua::types::ObjectId::ObjectsFolder;
use opcua::types::ReferenceTypeId::{HasProperty, Organizes};
use opcua::types::{
    AttributeId, DataEncoding, DataValue, MessageSecurityMode, NodeId, NodeSetNamespaceMapper,
    NumericRange, TimestampsToReturn, Variant,
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
/// Operation namespace URI.
const OPERATION_NS_URI: &str = "urn:traceability:operation";

/// Numeric node identifiers of the common namespace.
pub mod common_nid {
    pub const REQUEST: u32 = 1;
    pub const RESPONSE: u32 = 2;
    pub const HEARTBEAT: u32 = 3;
    pub const GENERAL_PART_SHEET: u32 = 1000;
    pub const PART_ID: u32 = 1001;
    pub const RAW_PART_REF: u32 = 1002;
    pub const RAW_BATCH: u32 = 1003;
}

/// Numeric node identifiers of the machine namespace.
mod machine_nid {
    pub(super) const OPERATION_PART_SHEET: u32 = 2000;
}

/// Nodeset importer for testing this application.
struct TestingNodes {
    namespaces: [&'static str; 2],
    operation_nodes: Vec<(&'static str, Variant)>,
}

impl TestingNodes {
    pub fn new_boxed(operation_nodes: Vec<(&'static str, Variant)>) -> Box<Self> {
        Box::new(Self {
            namespaces: [COMMON_NS_URI, OPERATION_NS_URI],
            operation_nodes,
        })
    }
}

impl NodeSetImport for TestingNodes {
    fn register_namespaces(&self, mapper: &mut NodeSetNamespaceMapper) {
        for (index, namespace) in (1..).zip(&self.namespaces) {
            mapper.add_namespace(namespace, index);
        }
    }

    fn get_own_namespaces(&self) -> Vec<String> {
        self.namespaces.map(|ns| ns.into()).to_vec()
    }

    fn load<'a>(
        &'a self,
        namespaces: &'a NodeSetNamespaceMapper,
    ) -> Box<dyn Iterator<Item = ImportedItem> + 'a> {
        let mut items = Vec::new();

        let common_ns_idx = namespaces
            .get_index(1)
            .expect("getting the traceability common namespace should not fail");

        // Nodes related to traceability protocol.
        let nodes: [(_, _, Variant); _] = [
            (common_nid::REQUEST, "TraceabilityRequest", 0u8.into()),
            (common_nid::RESPONSE, "TraceabilityResponse", 0u8.into()),
            (common_nid::HEARTBEAT, "TraceabilityHeartbeat", false.into()),
        ];
        for (nid, name, val) in nodes {
            items.push(ImportedItem {
                node: VariableBuilder::new(&NodeId::new(common_ns_idx, nid), name, name)
                    .data_type(val.data_type().unwrap().node_id)
                    .value(val)
                    .writable()
                    .build()
                    .into(),
                references: vec![ImportedReference {
                    target_id: ObjectsFolder.into(),
                    type_id: Organizes.into(),
                    is_forward: false,
                }],
            })
        }

        // General part sheet nodes.
        let general_part_sheet_node = NodeId::new(common_ns_idx, common_nid::GENERAL_PART_SHEET);
        items.push(ImportedItem {
            node: ObjectBuilder::new(
                &general_part_sheet_node,
                "TraceabilityPartSheetGeneral",
                "TraceabilityPartSheetGeneral",
            )
            .build()
            .into(),
            references: vec![ImportedReference {
                target_id: ObjectsFolder.into(),
                type_id: Organizes.into(),
                is_forward: false,
            }],
        });
        let nodes: [(_, _, Variant); _] = [
            (common_nid::PART_ID, "NumeUniq", "".into()),
            (common_nid::RAW_PART_REF, "RefePiecForg", "".into()),
            (common_nid::RAW_BATCH, "LotMati", "".into()),
            (1004, "RefePiecFini", "".into()),
            (1005, "PiecAnnoChgtRefPiecFini", false.into()),
            (1006, "PiecAnnoChgtLotMati", false.into()),
            (1007, "SuivTravPiecParPost", vec![false; 5].into()),
            (1008, "SuivConfPiecParPost", vec![false; 5].into()),
            (1009, "QuiDeclNonConf", "".into()),
            (1010, "QuiDeclConf", "".into()),
            (1011, "ProgReceMach", "".into()),
            (1012, "SuivPostAvecPassTrav", vec![false; 5].into()),
            (1013, "SuivPrelPiecParPost", vec![false; 5].into()),
            (1014, "SuivRejePiecParPost", vec![false; 5].into()),
            (1015, "RefePiecFiniInco", false.into()),
            (1016, "PiecTravInco", false.into()),
            (1017, "PiecNonConfAmon", false.into()),
            (1018, "LotMatiInco", false.into()),
            (1019, "PiecRebu", false.into()),
            (1020, "DeclRebuMoti", "".into()),
            (1021, "DeclRebuQui", "".into()),
        ];
        for (nid, name, val) in nodes {
            items.push(ImportedItem {
                node: VariableBuilder::new(&NodeId::new(common_ns_idx, nid), name, name)
                    .data_type(val.data_type().unwrap().node_id)
                    .value(val)
                    .writable()
                    .build()
                    .into(),
                references: vec![ImportedReference {
                    target_id: general_part_sheet_node.clone(),
                    type_id: HasProperty.into(),
                    is_forward: false,
                }],
            })
        }

        // Operation part sheet nodes.
        let operation_ns_idx = namespaces
            .get_index(2)
            .expect("getting the operation namespace index should not fail");
        let operation_part_sheet_node =
            NodeId::new(operation_ns_idx, machine_nid::OPERATION_PART_SHEET);
        items.push(ImportedItem {
            node: ObjectBuilder::new(
                &operation_part_sheet_node,
                "TraceabilityPartSheetOperation",
                "TraceabilityPartSheetOperation",
            )
            .build()
            .into(),
            references: vec![ImportedReference {
                target_id: ObjectsFolder.into(),
                type_id: Organizes.into(),
                is_forward: false,
            }],
        });
        for (nid, (name, val)) in (5000..).zip(self.operation_nodes.clone()) {
            items.push(ImportedItem {
                node: VariableBuilder::new(&NodeId::new(operation_ns_idx, nid), name, name)
                    .data_type(val.data_type().unwrap().node_id)
                    .value(val)
                    .writable()
                    .build()
                    .into(),
                references: vec![ImportedReference {
                    target_id: operation_part_sheet_node.clone(),
                    type_id: HasProperty.into(),
                    is_forward: false,
                }],
            })
        }

        Box::new(items.into_iter())
    }
}

/// Build and return traceability common OPC-UA configuration.
pub fn traceability_common_opcua_config() -> TraceabilityCommonOpcUaConfig {
    TraceabilityCommonOpcUaConfig {
        namespace_url: COMMON_NS_URI.into(),
        request_nid: common_nid::REQUEST,
        response_nid: common_nid::RESPONSE,
        heartbeat_nid: common_nid::HEARTBEAT,
        general_part_sheet_nid: common_nid::GENERAL_PART_SHEET,
        part_id_nid: common_nid::PART_ID,
        raw_part_ref_nid: common_nid::RAW_PART_REF,
        raw_batch_nid: common_nid::RAW_BATCH,
    }
}

/// Build and return traceability machine OPC-UA configuration.
pub fn traceability_machine_opcua_config() -> TraceabilityMachineOpcUaConfig {
    TraceabilityMachineOpcUaConfig {
        namespace_url: OPERATION_NS_URI.into(),
        operation_part_sheet_nid: machine_nid::OPERATION_PART_SHEET,
    }
}

/// Represents an OPC-UA server to be used in tests.
pub struct OpcUaTestServer {
    name: String,
    url: String,
    line_id: Option<AsciiDigitsOrUpper<2>>,
    node_manager: Arc<SimpleNodeManager>,
    common_ns_index: u16,
    server_handle: ServerHandle,
    task_handle: JoinHandle<Result<(), String>>,
}

impl OpcUaTestServer {
    /// Start and return an [`OpcUaTestServer`] and a channel receiver to signal
    /// that gateway has started to beat.
    pub async fn start(
        temp_dir: &Path,
        line_id: Option<AsciiDigitsOrUpper<2>>,
        operation_nodes: Vec<(&'static str, Variant)>,
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

        let (server, server_handle) = ServerBuilder::new()
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
            .add_user_token(
                "test_password_user",
                ServerUserToken {
                    user: "testuser".to_owned(),
                    pass: Some("testpassword".to_owned()),
                    ..Default::default()
                },
            )
            .add_endpoint(
                "None",
                ServerEndpoint::new_none(
                    "/",
                    &[ANONYMOUS_USER_TOKEN_ID.into(), "test_password_user".into()],
                ),
            )
            .add_endpoint(
                "Aes256Sha256RsaPss - Sign & Encrypt",
                ServerEndpoint::new_aes256_sha256_rsapss_sign_encrypt(
                    "/",
                    &[ANONYMOUS_USER_TOKEN_ID.into(), "test_password_user".into()],
                ),
            )
            .with_node_manager(simple_node_manager_imports(
                vec![TestingNodes::new_boxed(operation_nodes)],
                "testing-nodes-manager",
            ))
            .build()
            .expect("building the server should not fail");

        let node_manager = server_handle
            .node_managers()
            .get_of_type::<SimpleNodeManager>()
            .expect("node manager should exist");

        let common_ns_index = server_handle
            .get_namespace_index(COMMON_NS_URI)
            .expect("common nodes namespace index should exist");

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
        std::fs::copy(
            my_cert_file,
            gateway_pki_dir.join(format!("trusted/server_{}.der", local_addr.port())),
        )
        .expect("writing the server cert in gateway PKI should not fail");
        std::fs::copy(
            gateway_pki_dir.join("own/opcua-line-gateway-cert.der"),
            pki_dir.join("trusted/opcua-line-gateway-cert.der"),
        )
        .expect("copying gateway cert in server PKI should not fail");

        Self {
            name,
            url,
            line_id,
            node_manager,
            common_ns_index,
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
        OpcUaServerConfig {
            url: self.url.clone(),
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
        }
    }

    /// Return the credentials to use for authenticating with this server.
    pub fn opcua_credentials(&self) -> Option<Credential> {
        None
    }

    /// Wait for a client to have subscribed to the request code value.
    #[instrument(skip_all, fields(url = self.url))]
    pub async fn wait_request_subscription(&self) {
        info!("waiting for a client to subscribe to request code");

        let node_id = NodeId::new(self.common_ns_index, common_nid::REQUEST);
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
        .expect("waiting for request code subscription should not time out");
    }

    /// Write the value of a variable in the common namespace.
    pub fn write_common_node<V>(&self, nid: u32, value: V)
    where
        V: Into<Variant>,
    {
        let node_id = NodeId::new(self.common_ns_index, nid);
        self.node_manager
            .set_value(
                self.server_handle.subscriptions(),
                &node_id,
                None,
                DataValue::new_now(value),
            )
            .expect("writing a value in the common namespace should not fail");
    }

    /// Read the value of a variable in the common namespace.
    pub fn read_common_node(&self, nid: u32) -> Variant {
        let node_id = NodeId::new(self.common_ns_index, nid);
        let address_space = self.node_manager.address_space().read_arc();
        let node = address_space
            .find_node(&node_id)
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

    /// Wait for the response code to match the given predicate, and return it.
    pub async fn wait_response(&self, predicate: impl Fn(u8) -> bool) -> u8 {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let response = self.read_common_node(common_nid::RESPONSE);
                if let Variant::Byte(resp_code) = response
                    && predicate(resp_code)
                {
                    return resp_code;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("waiting for response should not time out")
    }

    /// Reset the request code and wait for the response reset.
    pub async fn traceability_reset(&self) {
        self.write_common_node(common_nid::REQUEST, 0u8);
        self.wait_response(|code| code == 0).await;
    }

    /// Run a full request cycle: write the request code, wait for a non-zero response,
    /// then reset the request and wait for the response reset. Returns the response code.
    pub async fn traceability_request(&self, code: u8) -> u8 {
        self.write_common_node(common_nid::REQUEST, code);
        let response = self.wait_response(|code| code != 0).await;
        self.traceability_reset().await;

        response
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
