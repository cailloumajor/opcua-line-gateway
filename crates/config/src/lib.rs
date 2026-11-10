use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use opcua::crypto::SecurityPolicy;
use opcua::types::MessageSecurityMode;
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

pub use self::ascii_text::{AsciiDigitsOrUpper, AsciiDigitsOrUpperError};
pub use self::credentials::{Credential, Credentials};
pub use self::node_name::{NodeName, NodeNameError};

mod ascii_text;
mod credentials;
mod foreign;
mod node_name;

/// Represents errors that can be encountered with configuration.
#[derive(Debug, Error)]
pub enum LineGatewayConfigError {
    #[error("error reading the configuration file")]
    ReadFile(#[source] io::Error),
    #[error(transparent)]
    ParseToml(toml::de::Error),
    #[error("no OPC-UA server configured, running would be pointless")]
    EmptyServers,
}

/// OPC-UA line gateway configuration.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineGatewayConfig {
    /// Globally unique identifier for the application instance, as of OPC-UA.
    pub application_uri: String,
    /// Root directory of the OPC-UA PKI.
    pub pki_dir: PathBuf,
    /// Traceability configuration for all machines.
    pub traceability: TraceabilityCommonConfig,
    /// Connected machines configuration, mapped by machine identifier.
    pub machines: BTreeMap<String, MachineConfig>,
}

impl LineGatewayConfig {
    /// Create the [`LineGatewayConfig`] from the provided path to a TOML file.
    pub fn from_toml_file<P>(path: P) -> Result<Self, LineGatewayConfigError>
    where
        P: AsRef<Path>,
    {
        let raw = fs::read_to_string(path).map_err(LineGatewayConfigError::ReadFile)?;

        let config = toml::from_str::<Self>(&raw).map_err(LineGatewayConfigError::ParseToml)?;

        // Validate that we have at least one server configured.
        if config.machines.is_empty() {
            return Err(LineGatewayConfigError::EmptyServers);
        }

        Ok(config)
    }
}

/// Traceability configuration for all machines.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceabilityCommonConfig {
    /// Two character production line identifier for traceability.
    pub line_id: AsciiDigitsOrUpper<2>,
    /// Path to the redb file to use for traceability cache. It will be created
    /// if it does not exist.
    pub redb_file: PathBuf,
    /// ClickHouse database client configuration for archiving traceability data.
    pub database: TraceabilityDatabaseConfig,
    /// The upper limit of enqueued part sheets to consider draining as working.
    pub queues_not_draining_threshold: u64,
    /// OPC-UA traceability information model, common to all machines.
    pub opc_ua: TraceabilityCommonOpcUaConfig,
}

/// OPC-UA traceability information model, as implemented by all mailboxes of all
/// machines.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceabilityCommonOpcUaConfig {
    /// OPC-UA namespace URL used for common part of traceability.
    #[schemars(url)]
    pub namespace_url: String,
    /// OPC-UA `BrowseName` of the traceability heartbeat node.
    pub heartbeat_node: String,
    /// OPC-UA `BrowseName` of the traceability mailbox common type.
    pub mailbox_type: String,
    /// OPC-UA `BrowseName` of the traceability request node.
    pub request_node: String,
    /// OPC-UA `BrowseName` of the traceability response node.
    pub response_node: String,
    /// OPC-UA `BrowseName` of the general part sheet object.
    pub general_part_sheet_node: String,
    /// OPC-UA `BrowseName` of the operation part sheet common type.
    pub operation_part_sheet_type: String,
    /// OPC-UA `BrowseName` of the part identifier variable.
    pub part_id_node: NodeName,
    /// OPC-UA `BrowseName` of the raw part reference variable.
    pub raw_part_ref_node: NodeName,
    /// OPC-UA `BrowseName` of the raw material batch variable.
    pub raw_batch_node: NodeName,
    /// OPC-UA `BrowseName` of the nodes to be excluded from general part sheet read request.
    pub excluded_read_nodes: Vec<NodeName>,
}

/// ClickHouse database configuration for traceability.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceabilityDatabaseConfig {
    /// URL of the ClickHouse HTTP(S) endpoint.
    #[schemars(url)]
    pub url: String,
    /// Default database to use.
    pub default_database: String,
    /// Part sheets draining task execution period.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    #[schemars(with = "String")]
    pub part_sheets_drain_period: Duration,
    /// Table to use for general part sheet.
    pub general_part_sheet_table: String,
    /// Table to use for operation part sheet.
    pub operation_part_sheet_table: String,
}

/// Connected machine configuration.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MachineConfig {
    /// OPC-UA server configuration for this machine.
    pub opc_ua_server: OpcUaServerConfig,
    /// Traceability settings for this machine.
    pub traceability: TraceabilityMachineConfig,
}

/// Connected OPC-UA server configuration.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpcUaServerConfig {
    /// OPC-UA server URL.
    #[schemars(url)]
    pub url: String,
    /// OPC-UA security policy.
    #[serde(with = "foreign::SecurityPolicy")]
    pub security_policy: SecurityPolicy,
    /// OPC-UA security mode.
    #[serde(with = "foreign::MessageSecurityMode")]
    pub security_mode: MessageSecurityMode,
    /// Publish interval for OPC-UA subscription to request variable.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    #[schemars(with = "String")]
    pub publish_interval: Duration,
}

/// Traceability related configuration for a machine.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceabilityMachineConfig {
    /// OPC-UA namespace URL used for operation part of traceability.
    #[schemars(url)]
    pub namespace_url: String,
    /// OPC-UA node identifier of the machine object (root traceability object).
    pub machine_nid: u32,
}
