use std::path::{Path, PathBuf};
use std::time::Duration;

use opcua::crypto::CertificateStore;
use opcua::types::{ApplicationDescription, ApplicationType};
use opcua_line_gateway::{TimeProvider, run_main};
use opcua_line_gateway_config::{
    Credential, Credentials, LineGatewayConfig, MachineConfig, TraceabilityCommonConfig,
    TraceabilityDatabaseConfig, TraceabilityMachineConfig,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use super::opcua_server::{
    OpcUaTestServer, traceability_common_opcua_config, traceability_machine_opcua_config,
};

const APPLICATION_URI: &str = "urn:line-gateway:test";

/// Return the path to the PKI for the gateway, provided the temporary path.
pub fn gateway_pki_dir(temp_dir: &Path) -> PathBuf {
    temp_dir.join("gateway_pki")
}

/// Create the PKI for the gateway.
#[instrument(skip_all)]
pub fn create_gateway_pki(temp_dir: &Path) {
    let app_description = ApplicationDescription {
        application_uri: APPLICATION_URI.into(),
        product_uri: concat!("urn:", env!("CARGO_PKG_NAME")).into(),
        application_name: env!("CARGO_PKG_DESCRIPTION").into(),
        application_type: ApplicationType::Client,
        gateway_server_uri: Default::default(),
        discovery_profile_uri: Default::default(),
        discovery_urls: None,
    };

    let cert_path = Path::new(concat!("own/", env!("CARGO_PKG_NAME"), "-cert.der"));
    let pkey_path = Path::new(concat!("private/", env!("CARGO_PKG_NAME"), "-key.pem"));
    let (_, cert, key) = CertificateStore::new_with_x509_data(
        &gateway_pki_dir(temp_dir),
        false,
        Some(cert_path),
        Some(pkey_path),
        app_description.into(),
    );
    assert!(
        cert.zip(key).is_some(),
        "gateway cert and key should have been created"
    );
}

/// Represents a running gateway to be used in tests.
pub struct TestGateway {
    shutdown: CancellationToken,
    task_handle: JoinHandle<anyhow::Result<()>>,
}

impl TestGateway {
    /// Start a gateway connected to the OPC-UA server at the given URL.
    #[instrument(skip_all)]
    pub async fn start(
        temp_dir: &Path,
        time_provider: TimeProvider,
        db_config: TraceabilityDatabaseConfig,
        db_creds: Credential,
        servers: &[&OpcUaTestServer],
    ) -> Self {
        let machines = servers
            .iter()
            .map(|server| {
                (
                    server.name().into(),
                    MachineConfig {
                        opc_ua_server: server.opcua_config(),
                        traceability: TraceabilityMachineConfig {
                            publish_interval: Duration::from_millis(10),
                            line_id: server.line_id(),
                            opc_ua: traceability_machine_opcua_config(),
                        },
                    },
                )
            })
            .collect();

        let config = LineGatewayConfig {
            application_uri: APPLICATION_URI.into(),
            pki_dir: gateway_pki_dir(temp_dir),
            traceability: TraceabilityCommonConfig {
                redb_file: temp_dir.join("test.redb"),
                queues_not_draining_threshold: 10,
                opc_ua: traceability_common_opcua_config(),
                database: db_config,
            },
            machines,
        };

        let opcua_creds = servers
            .iter()
            .filter_map(|server| {
                server
                    .opcua_credentials()
                    .map(|cred| (server.name().into(), cred))
            })
            .collect();

        let credentials = Credentials {
            database: db_creds,
            opc_ua: opcua_creds,
        };

        let shutdown = CancellationToken::new();
        let task_handle = tokio::spawn(run_main(
            config,
            credentials,
            time_provider,
            shutdown.clone(),
        ));

        // Wait for request subscription on all servers.
        for server in servers {
            server.wait_request_subscription().await;
        }

        Self {
            shutdown,
            task_handle,
        }
    }

    /// Stop the gateway and wait for it to have finished its work.
    pub async fn stop(self) {
        self.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(2), self.task_handle)
            .await
            .expect("waiting for gateway task end should not time out")
            .expect("joining the gateway task should not fail")
            .expect("gateway task should not fail");
    }
}
