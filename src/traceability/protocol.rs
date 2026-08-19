use strum::FromRepr;

/// The response code for reset state.
pub(super) const RESPONSE_RESET: u8 = 0;
/// The response code for success state.
pub(super) const RESPONSE_SUCCESS: u8 = 1;

/// Traceability request code.
#[derive(Clone, Copy, FromRepr)]
#[repr(u8)]
pub(super) enum TraceabilityRequest {
    /// Reset state of the request.
    Reset = 0,
    /// Request for creating a part ID.
    CreatePartId = 1,
    /// Request from the machine to read part data sheet.
    ReadPartSheet = 2,
    /// Request from the machine to save part data sheets.
    SavePartSheets = 3,
}
