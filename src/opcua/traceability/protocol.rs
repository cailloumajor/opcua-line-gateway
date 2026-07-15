use opcua::types::{IntoVariant, Variant};
use strum::FromRepr;

/// Traceability request code.
#[derive(Clone, Copy, FromRepr)]
#[repr(u8)]
pub(super) enum TraceabilityRequest {
    /// Reset state of the request.
    Reset = 0,
    /// Request for creating a part ID.
    CreatePartId = 1,
    /// Request from the machine for getting part data sheets.
    GetPartSheets = 2,
    /// Request from the machine for saving part data sheets.
    SavePartSheets = 3,
}

/// Traceability response code.
#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum TraceabilityResponse {
    /// Reset state of the response.
    Reset = 0,

    ErrorValueMissing = 10,
    ErrorInvalidValue = 11,
    ErrorUnknownValue = 12,
}

impl IntoVariant for TraceabilityResponse {
    fn into_variant(self) -> Variant {
        (self as u8).into()
    }
}
