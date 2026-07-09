use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

use opcua::crypto::SecurityPolicy;
use opcua::types::MessageSecurityMode;
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

mod foreign;

/// Represents errors that can be encountered with configuration.
#[derive(Debug, Error)]
pub enum LineGatewayConfigError {
    #[error("error reading the configuration file")]
    ReadFile(#[source] io::Error),
    #[error(transparent)]
    ParseToml(toml::de::Error),
    #[error("no OPC-UA server configured, running would be pointless")]
    EmptyServers,
    #[error("missing OPC-UA username for `{0}` server configuration")]
    MissingUsername(String),
    #[error("missing OPC-UA password for `{0}` server configuration")]
    MissingPassword(String),
}

/// The configuration for an OPC-UA server to communicate with.
#[derive(Deserialize, JsonSchema)]
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
    /// The username if authenticating to the OPC-UA server with username/password.
    /// If not provided, anonymous authentication will be used.
    pub user: Option<String>,
    /// The password to use if using username/password authentication.
    pub password: Option<String>,
}

/// OPC-UA line gateway configuration.
#[derive(Deserialize, JsonSchema)]
pub struct LineGatewayConfig {
    /// The globally unique identifier for the application instance, as of OPC-UA.
    pub application_uri: String,
    /// The root directory of the OPC-UA PKI.
    pub pki_dir: PathBuf,
    /// Mapping of machine identifier to corresponding OPC-UA server configuration.
    pub opcua_servers: BTreeMap<String, OpcUaServerConfig>,
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
        if config.opcua_servers.is_empty() {
            return Err(LineGatewayConfigError::EmptyServers);
        }

        // Validate OPC-UA username and password.
        for (server_id, server_config) in &config.opcua_servers {
            match (&server_config.user, &server_config.password) {
                (None, Some(_)) => {
                    return Err(LineGatewayConfigError::MissingUsername(server_id.clone()));
                }
                (Some(_), None) => {
                    return Err(LineGatewayConfigError::MissingPassword(server_id.clone()));
                }
                _ => {}
            }
        }

        Ok(config)
    }
}
