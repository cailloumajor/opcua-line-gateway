use std::collections::BTreeMap;
use std::path::Path;
use std::{env, fs};

use anyhow::Context as _;
use serde::Deserialize;

/// A credential, i.e. username and password.
#[derive(Clone, Deserialize)]
pub(crate) struct Credential {
    pub(crate) user: String,
    pub(crate) password: String,
}

/// Credentials used through the application.
#[derive(Deserialize)]
pub(super) struct Credentials {
    /// Database credential.
    pub(super) database: Credential,
    /// OPC-UA servers credentials, mapping server identifier to credential.
    pub(super) opc_ua: BTreeMap<String, Credential>,
}

impl Credentials {
    /// Builds the credentials object from a TOML credentials file.
    ///
    /// The file is read from `$CREDENTIALS_DIRECTORY/credentials.toml`, where
    /// `$CREDENTIALS_DIRECTORY` is expected to be set by the
    /// [systemd credentials feature] or a similar mechanism.
    ///
    /// [systemd credentials feature]: https://systemd.io/CREDENTIALS/
    pub(super) fn from_credentials_file() -> anyhow::Result<Self> {
        let creds_dir = env::var_os("CREDENTIALS_DIRECTORY")
            .context("Failed to get CREDENTIALS_DIRECTORY environment variable value")?;
        let creds_path = Path::new(&creds_dir)
            .join("credentials.toml")
            .with_extension("toml");
        let file_contents = fs::read_to_string(&creds_path)
            .with_context(|| format!("Failed to read {} file", creds_path.display()))?;
        toml::from_str(&file_contents).context("Failed to deserialize file contents")
    }
}
