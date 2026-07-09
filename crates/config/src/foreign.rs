use schemars::JsonSchema;
use serde::Deserialize;

/// A copy of [`opcua::crypto::SecurityPolicy`] to allow using serde remote functionality.
#[derive(Deserialize, JsonSchema)]
#[serde(remote = "opcua::crypto::SecurityPolicy")]
pub(crate) enum SecurityPolicy {
    /// This member represents an invalid security policy, so forbid deserializing it.
    #[serde(skip_deserializing)]
    Unknown,
    None,
    Aes128Sha256RsaOaep,
    Basic256Sha256,
    Aes256Sha256RsaPss,
    Basic128Rsa15,
    Basic256,
}

/// A copy of [`opcua::crypto::MessageSecurityMode`] to allow using serde remote functionality.
#[derive(Deserialize, JsonSchema)]
#[serde(remote = "opcua::types::MessageSecurityMode")]
pub(crate) enum MessageSecurityMode {
    /// This member represents an invalid security mode, so forbid deserializing it.
    #[serde(skip_deserializing)]
    Invalid,
    None,
    Sign,
    SignAndEncrypt,
}
