use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use opcua::client::IdentityToken;
use opcua::crypto::SecurityPolicy;
use opcua::types::{EndpointDescription, MessageSecurityMode};
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

pub use self::ascii_text::{AsciiText, AsciiTextError};

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
    pub traceability: CommonTraceabilityConfig,
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

        // Validate database password file permissions.
        let password_file_metadata = fs::metadata(&config.traceability.database.password_file)
            .map_err(LineGatewayConfigError::DbPassFileMeta)?;
        let password_file_mode = password_file_metadata.permissions().mode() & 0o7777;
        if password_file_mode != 0o600 {
            return Err(LineGatewayConfigError::DbPassFilePermissions(
                password_file_mode,
            ));
        }

        // Validate that we have at least one server configured.
        if config.machines.is_empty() {
            return Err(LineGatewayConfigError::EmptyServers);
        }

        // Validate OPC-UA username and password.
        for (machine_id, machine_config) in &config.machines {
            match (
                &machine_config.opc_ua_server.user,
                &machine_config.opc_ua_server.password,
            ) {
                (None, Some(_)) => {
                    return Err(LineGatewayConfigError::MissingUsername(machine_id.clone()));
                }
                (Some(_), None) => {
                    return Err(LineGatewayConfigError::MissingPassword(machine_id.clone()));
                }
                _ => {}
            }
        }

        Ok(config)
    }
}

/// Traceability configuration for all machines.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct CommonTraceabilityConfig {
    /// Path to the redb file to use for traceability cache. It will be created
    /// if it does not exist.
    pub redb_file: PathBuf,
    /// ClickHouse database client configuration for archiving traceability data.
    pub database: TraceabilityDatabaseConfig,
}

/// ClickHouse database configuration for traceability.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityDatabaseConfig {
    /// URL of the ClickHouse HTTP(S) endpoint.
    #[schemars(url)]
    pub url: String,
    /// ClickHouse user.
    pub user: String,
    /// Path to a file containing the ClickHouse user's password. Whitespaces around
    /// the password will be removed.
    pub password_file: PathBuf,
    /// Default database to use.
    pub default_database: String,
    /// Table to use for general part sheet.
    pub general_part_sheet_table: String,
}

/// Connected machine configuration.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct MachineConfig {
    /// OPC-UA server configuration for this machine.
    pub opc_ua_server: OpcUaServerConfig,
    /// Traceability settings for this machine.
    pub traceability: MachineTraceabilityConfig,
}

/// Connected OPC-UA server configuration.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct OpcUaServerConfig {
    /// OPC-UA server URL.
    #[schemars(url)]
    pub url: String,
    /// OPC_UA security policy.
    #[serde(with = "foreign::SecurityPolicy")]
    pub security_policy: SecurityPolicy,
    /// OPC-UA security mode.
    #[serde(with = "foreign::MessageSecurityMode")]
    pub security_mode: MessageSecurityMode,
    /// Username if authenticating to the OPC-UA server with username/password.
    /// If not provided, anonymous authentication will be used.
    pub user: Option<String>,
    /// Password to use if using username/password authentication.
    pub password: Option<String>,
}

impl OpcUaServerConfig {
    /// Create an [`EndpointDescription`] from this server configuration.
    pub fn endpoint_description(&self) -> EndpointDescription {
        EndpointDescription::from((
            self.url.as_str(),
            self.security_policy.to_str(),
            self.security_mode,
        ))
    }

    /// Create an [`IdentityToken`] from this server configuration.
    pub fn identity_token(&self) -> IdentityToken {
        self.user
            .as_ref()
            .zip(self.password.as_ref())
            .map(|(user, pass)| IdentityToken::new_user_name(user, pass))
            .unwrap_or(IdentityToken::new_anonymous())
    }
}

/// Traceability related configuration for a machine.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct MachineTraceabilityConfig {
    /// OPC-UA namespace URL used for traceability.
    #[schemars(url)]
    pub namespace_url: String,
    /// Publish interval for OPC-UA subscription to request variable.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    #[schemars(with = "String")]
    pub publish_interval: Duration,
    /// Traceability-related OPC-UA nodes.
    pub nodes: TraceabilityOpcUaNodesConfig,
    /// Configuration for part identifier creation, if applicable.
    pub part_identifier: Option<CreatePartIdConfig>,
}

/// OPC-UA nodes used for traceability.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct TraceabilityOpcUaNodesConfig {
    /// OPC-UA node identifier of the request variable.
    pub request: u32,
    /// OPC-UA node identifier of the response variable.
    pub response: u32,
    /// OPC-UA node identifier of the heartbeat variable.
    pub heartbeat: u32,
    /// OPC-UA node identifier of the general part sheet object.
    pub general_part_sheet: u32,
    /// OPC-UA node identifier of the `part ID` variable.
    pub part_id: u32,
}

/// Configuration related to part ID creation for a machine.
#[derive(Clone, Deserialize, JsonSchema)]
pub struct CreatePartIdConfig {
    /// OPC-UA node identifier of the raw part reference variable.
    pub raw_part_ref_node: u32,
    /// OPC-UA node identifier of the raw material batch variable.
    pub raw_batch_node: u32,
    /// Two character production line identifier.
    pub line_id: AsciiText<2>,
}
