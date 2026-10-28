use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use serde::Deserialize;
use thiserror::Error;

/// Name of environment variable holding credentials directory.
const CRED_DIR_ENV_VAR: &str = "CREDENTIALS_DIRECTORY";

/// Errors that can occur during getting credentials from file.
#[derive(Debug, Error)]
pub enum CredentialsFromFileError {
    #[error("missing `{CRED_DIR_ENV_VAR}` environment variable")]
    EnvVar,
    #[error("error reading file {1}")]
    FileRead(#[source] io::Error, PathBuf),
    #[error(transparent)]
    Toml(toml::de::Error),
}

/// A credential, i.e. username and password.
#[derive(Clone, Deserialize)]
pub struct Credential {
    pub user: String,
    pub password: String,
}

/// Credentials used through the application.
#[derive(Deserialize)]
pub struct Credentials {
    /// Database credential.
    pub database: Credential,
    /// OPC-UA servers credentials, mapping server identifier to credential.
    #[serde(default)]
    pub opc_ua: BTreeMap<String, Credential>,
}

impl Credentials {
    /// Builds the credentials object from a TOML credentials file.
    ///
    /// The file is read from `$CREDENTIALS_DIRECTORY/credentials.toml`, where
    /// `$CREDENTIALS_DIRECTORY` is expected to be set by the
    /// [systemd credentials feature] or a similar mechanism.
    ///
    /// [systemd credentials feature]: https://systemd.io/CREDENTIALS/
    pub fn from_credentials_file() -> Result<Self, CredentialsFromFileError> {
        let creds_dir = env::var_os(CRED_DIR_ENV_VAR).ok_or(CredentialsFromFileError::EnvVar)?;
        let creds_path = Path::new(&creds_dir)
            .join("credentials.toml")
            .with_extension("toml");
        Self::from_toml_file(creds_path)
    }

    /// Create the [`Credentials`] from the provided path to a TOML file.
    pub fn from_toml_file<P>(path: P) -> Result<Self, CredentialsFromFileError>
    where
        P: AsRef<Path>,
    {
        let file_contents = fs::read_to_string(&path)
            .map_err(|err| CredentialsFromFileError::FileRead(err, path.as_ref().to_owned()))?;
        toml::from_str(&file_contents).map_err(CredentialsFromFileError::Toml)
    }
}
