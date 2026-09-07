use schemars::JsonSchema;
use serde::Deserialize;

/// A copy of [`opcua::crypto::SecurityPolicy`] to allow using serde remote functionality.
///
/// The schema is inlined so that the possible values are documented directly on the
/// configuration member instead of a reference to a separate definition.
#[derive(Deserialize, JsonSchema)]
#[schemars(inline)]
#[serde(remote = "opcua::crypto::SecurityPolicy")]
pub(crate) enum SecurityPolicy {
    // This member represents an invalid security policy, so forbid deserializing it. Note that
    // this is not a doc comment on purpose: it would make `schemars` generate a `oneOf` schema
    // instead of a plain string enum, which documentation generators fail to render.
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
///
/// The schema is inlined so that the possible values are documented directly on the
/// configuration member instead of a reference to a separate definition.
#[derive(Deserialize, JsonSchema)]
#[schemars(inline)]
#[serde(remote = "opcua::types::MessageSecurityMode")]
pub(crate) enum MessageSecurityMode {
    // This member represents an invalid security mode, so forbid deserializing it. Note that
    // this is not a doc comment on purpose: it would make `schemars` generate a `oneOf` schema
    // instead of a plain string enum, which documentation generators fail to render.
    #[serde(skip_deserializing)]
    Invalid,
    None,
    Sign,
    SignAndEncrypt,
}
