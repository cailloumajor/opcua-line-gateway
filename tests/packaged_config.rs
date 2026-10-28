//! Checks that the example configuration files shipped in the Debian package stay valid.

use std::path::PathBuf;

use opcua_line_gateway_config::{Credentials, LineGatewayConfig};

#[test]
fn config_example_is_valid() {
    let config_file = PathBuf::from_iter([env!("CARGO_MANIFEST_DIR"), "debian", "config.toml"]);
    LineGatewayConfig::from_toml_file(config_file)
        .expect("parsing the packaged configuration should not fail");
}

#[test]
fn credentials_example_is_valid() {
    let creds_file = PathBuf::from_iter([env!("CARGO_MANIFEST_DIR"), "debian", "credentials.toml"]);
    Credentials::from_toml_file(creds_file)
        .expect("parsing the packaged credentials should not fail");
}
