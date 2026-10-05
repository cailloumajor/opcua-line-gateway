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

mod ascii_text;
mod foreign;

/// Represents errors that can be encountered with configuration.
#[derive(Debug, Error)]
pub enum LineGatewayConfigError {
    #[error("error reading the configuration file")]
    ReadFile(#[source] io::Error),
    #[error(transparent)]
    ParseToml(toml::de::Error),
    #[error("error getting database password file metadata")]
    DbPassFileMeta(#[source] io::Error),
    #[error("invalid database password file permissions (expected '0600', got '{0:04o}')")]
    DbPassFilePermissions(u32),
    #[error("no OPC-UA server configured, running would be pointless")]
    EmptyServers,
    #[error("missing OPC-UA username for `{0}` server configuration")]
    MissingUsername(String),
    #[error("missing OPC-UA password for `{0}` server configuration")]
    MissingPassword(String),
}

/// OPC-UA line gateway configuration.
#[derive(Clone, Deserialize, JsonSchema)]
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
        let file_contents = fs::read_to_string(path).map_err(LineGatewayConfigError::ReadFile)?;
        let config =
            toml::from_str::<Self>(&file_contents).map_err(LineGatewayConfigError::ParseToml)?;

        // Validate that we have at least one server configured.
        if config.machines.is_empty() {
            return Err(LineGatewayConfigError::EmptyServers);
        }

        Ok(config)
    }
}

/// Traceability configuration for all machines.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityCommonConfig {
    /// Path to the redb file to use for traceability cache. It will be created
    /// if it does not exist.
    pub redb_file: PathBuf,
    /// The upper limit of enqueued part sheets to consider draining as working.
    pub queues_not_draining_threshold: u64,
    /// OPC-UA traceability information model, common to all machines.
    pub opc_ua: TraceabilityCommonOpcUaConfig,
    /// ClickHouse database client configuration for archiving traceability data.
    pub database: TraceabilityDatabaseConfig,
}

/// OPC-UA traceability information model, as implemented by all machines.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityCommonOpcUaConfig {
    /// OPC-UA namespace URL used for common part of traceability.
    #[schemars(url)]
    pub namespace_url: String,
    /// OPC-UA node identifier of the request variable.
    pub request_nid: u32,
    /// OPC-UA node identifier of the response variable.
    pub response_nid: u32,
    /// OPC-UA node identifier of the heartbeat variable.
    pub heartbeat_nid: u32,
    /// OPC-UA node identifier of the general part sheet object.
    pub general_part_sheet_nid: u32,
    /// OPC-UA node identifier of the `part ID` variable.
    pub part_id_nid: u32,
    /// OPC-UA node identifier of the raw part reference variable.
    pub raw_part_ref_nid: u32,
    /// OPC-UA node identifier of the raw material batch variable.
    pub raw_batch_nid: u32,
}

/// ClickHouse database configuration for traceability.
#[derive(Clone, Deserialize, JsonSchema)]
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
pub struct MachineConfig {
    /// OPC-UA server configuration for this machine.
    pub opc_ua_server: OpcUaServerConfig,
    /// Traceability settings for this machine.
    pub traceability: TraceabilityMachineConfig,
}

/// Connected OPC-UA server configuration.
#[derive(Clone, Deserialize, JsonSchema)]
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
}

/// Traceability related configuration for a machine.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityMachineConfig {
    /// Publish interval for OPC-UA subscription to request variable.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    #[schemars(with = "String")]
    pub publish_interval: Duration,
    /// Two character production line identifier, only present if the machine makes
    /// part identifier creation requests.
    pub line_id: Option<AsciiDigitsOrUpper<2>>,
    /// OPC-UA traceability information model for this machine.
    pub opc_ua: TraceabilityMachineOpcUaConfig,
}

/// OPC-UA traceability information model, specific for each machine.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityMachineOpcUaConfig {
    /// OPC-UA namespace URL used for operation part of traceability.
    #[schemars(url)]
    pub namespace_url: String,
    /// OPC-UA node identifier of the operation part sheet object.
    pub operation_part_sheet_nid: u32,
}
