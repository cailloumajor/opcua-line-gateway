use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

/// Represents errors that can be encountered with configuration.
#[derive(Debug, Error)]
pub enum LineGatewayConfigError {
    #[error("error reading the configuration file")]
    ReadFile(#[source] io::Error),
    #[error("error parsing configuration TOML")]
    ParseToml(#[source] toml::de::Error),
}

/// The configuration for an OPC-UA server to communicate with.
#[derive(Deserialize, JsonSchema)]
pub struct OpcUaServerConfig {}

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
        let config = toml::from_str(&file_contents).map_err(LineGatewayConfigError::ParseToml)?;

        Ok(config)
    }
}
