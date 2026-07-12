use std::error::Error;
use std::fmt;
use std::time::Duration;

use opcua::client::{Client, ClientBuilder};
use opcua_line_gateway_config::LineGatewayConfig;

/// Maximum time allowed for server requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

/// Maximum time allowed for publish requests to the server.
const PUBLISH_TIMEOUT: Duration = Duration::from_millis(200);

/// Wraps multiple errors, as returned by [`ClientBuilder::client()`]
#[derive(Debug)]
pub(crate) struct ClientBuildError(Vec<String>);

impl fmt::Display for ClientBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let build_error = self
            .0
            .first()
            .map(|s| s.as_str())
            .unwrap_or("unknown error (empty error list from ClientBuilder)");
        write!(f, "error building the OPC-UA client: {build_error}")
    }
}

impl Error for ClientBuildError {}

/// Create an OPC-UA [`Client`], provided the PKI root directory.
///
/// # Errors
///
/// An error is returned if something goes wrong building the [`Client`]. The message
/// of the returned error will be the first one from [`ClientBuilder::client()`] errors.
pub(crate) fn create_client(config: &LineGatewayConfig) -> Result<Client, ClientBuildError> {
    ClientBuilder::new()
        .application_uri(&config.application_uri)
        .product_uri(concat!("urn:", env!("CARGO_PKG_NAME")))
        .application_name(env!("CARGO_PKG_DESCRIPTION"))
        .pki_dir(&config.pki_dir)
        .certificate_path(concat!("own/", env!("CARGO_PKG_NAME"), "-cert.der"))
        .private_key_path(concat!("private/", env!("CARGO_PKG_NAME"), "-key.pem"))
        // Retry to re-establish the session forever.
        .session_retry_limit(-1)
        .request_timeout(REQUEST_TIMEOUT)
        .publish_timeout(PUBLISH_TIMEOUT)
        .client()
        .map_err(ClientBuildError)
}
