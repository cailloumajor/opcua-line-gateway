use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;
use url::{ParseError, Url};

/// The OPC-UA URL scheme we expect.
const OPCUA_SCHEME: &str = "opc.tcp";

/// Errors that can occur with [`OpcUaEndpointUrl`].
#[derive(Debug, Error)]
pub enum OpcUaEndpointUrlError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("invalid URL scheme (expected {scheme})", scheme = OPCUA_SCHEME)]
    Scheme,
}

/// Validated OPC-UA endpoint URL.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
pub struct OpcUaEndpointUrl(String);

impl OpcUaEndpointUrl {
    /// Get a string slice of the URL.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for OpcUaEndpointUrl {
    type Error = OpcUaEndpointUrlError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = Url::parse(&value)?;
        if url.scheme() != OPCUA_SCHEME {
            return Err(OpcUaEndpointUrlError::Scheme);
        }

        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn invalid_url() {
        let res = OpcUaEndpointUrl::try_from("opc.tcp://example.com:port".to_owned());

        assert_matches!(
            res,
            Err(OpcUaEndpointUrlError::Parse(ParseError::InvalidPort))
        );
    }

    #[test]
    fn bad_scheme() {
        let res = OpcUaEndpointUrl::try_from("http://example.com:4840".to_owned());

        assert_matches!(res, Err(OpcUaEndpointUrlError::Scheme));
    }

    #[test]
    fn success() {
        let url = OpcUaEndpointUrl::try_from("opc.tcp://exaMple.com:4840".to_owned())
            .expect("building OPC-UA endpoint URL should not fail");

        assert_eq!(url.as_str(), "opc.tcp://exaMple.com:4840");
    }
}
