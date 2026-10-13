use opcua::types::{DataValue, ReadValueId, TimestampsToReturn};
use strum::FromRepr;
use thiserror::Error;
use tracing::{info, instrument, warn};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};

use super::create_part_id::CreatePartIdError;
use super::read_part_sheet::HandleReadError;
use super::save_part_sheets::HandleSaveError;
use super::{TraceabilityContext, TraceabilityHandler};

/// The response code for reset state.
const RESPONSE_RESET: u8 = 0;
/// The response code for success state.
const RESPONSE_SUCCESS: u8 = 1;

/// Traceability request code.
#[derive(Clone, Copy, FromRepr)]
#[repr(u8)]
enum TraceabilityRequest {
    /// Reset state of the request.
    Reset = 0,
    /// Request for creating a part ID.
    CreatePartId = 1,
    /// Request from the machine to read part data sheet.
    ReadPartSheet = 2,
    /// Request from the machine to save part data sheets.
    SavePartSheets = 3,
}

/// Errors that can be encountered during request handling.
#[derive(Debug, Error)]
pub(super) enum HandleRequestError {
    #[error("invalid request value, cause: {0}")]
    ValueError(TryFromOpcUaValueError),
    #[error("unknown request value: {0}")]
    UnknownValue(u8),
    #[error("error creating the part ID")]
    CreatePartId(#[from] CreatePartIdError),
    #[error("error handling read request")]
    HandleRead(#[from] HandleReadError),
    #[error("error handling save request")]
    HandleSave(#[from] HandleSaveError),
}

impl HandleRequestError {
    /// Convert a request handling error to a traceability response code. This is intended
    /// to be used to generate a response code to write to the OPC-UA server in case
    /// of failure.
    pub(super) fn to_response_code(&self) -> u8 {
        match self {
            Self::ValueError(_) => 91,
            Self::UnknownValue(_) => 92,

            Self::CreatePartId(CreatePartIdError::NotConfigured) => 11,
            // Group the two following errors in the same error code.
            Self::CreatePartId(CreatePartIdError::ReadVariables(_)) => 12,
            Self::CreatePartId(CreatePartIdError::ReadLength(_, _)) => 12,
            Self::CreatePartId(CreatePartIdError::PartRefValue(_)) => 13,
            Self::CreatePartId(CreatePartIdError::BatchValue(_)) => 14,
            Self::CreatePartId(CreatePartIdError::NextSerial(_)) => 15,
            Self::CreatePartId(CreatePartIdError::PartIdentifier(_)) => 16,
            Self::CreatePartId(CreatePartIdError::WritePartId(_)) => 17,

            Self::HandleRead(HandleReadError::ReadPartId(_)) => 21,
            Self::HandleRead(HandleReadError::PartIdValue(_)) => 22,
            // Group the two following errors in the same error code.
            Self::HandleRead(HandleReadError::CacheGet(_)) => 23,
            Self::HandleRead(HandleReadError::CacheGetTask(_)) => 23,
            Self::HandleRead(HandleReadError::CacheMissing(_)) => 24,
            Self::HandleRead(HandleReadError::WritePartSheet(_)) => 25,

            // Group the two following errors in the same error code.
            Self::HandleSave(HandleSaveError::ReadPartSheets(_)) => 31,
            Self::HandleSave(HandleSaveError::ReadLength(_, _)) => 31,
            Self::HandleSave(HandleSaveError::PartSheetValue(_, _)) => 32,
            // Group the two following errors in the same error code.
            Self::HandleSave(HandleSaveError::PartIdValue(_)) => 33,
            Self::HandleSave(HandleSaveError::InvalidPartId(_, _)) => 33,
            // Group the two following errors in the same error code.
            Self::HandleSave(HandleSaveError::CacheSave(_)) => 34,
            Self::HandleSave(HandleSaveError::CacheSaveTask(_)) => 34,
            Self::HandleSave(HandleSaveError::Enqueuing(_)) => 35,
        }
    }
}

impl TraceabilityHandler<TraceabilityContext> {
    /// Handle a request code from the OPC-UA server. Upon success, return the response code
    /// that must be written to the server.
    #[instrument(err, skip_all)]
    pub(super) async fn handle_request(&self, value: DataValue) -> Result<u8, HandleRequestError> {
        let request_code = value
            .try_ua_value_as()
            .map_err(HandleRequestError::ValueError)?;
        let Some(req) = TraceabilityRequest::from_repr(request_code) else {
            return Err(HandleRequestError::UnknownValue(request_code));
        };

        match req {
            TraceabilityRequest::Reset => {
                info!(msg = "reset response code");

                return Ok(RESPONSE_RESET);
            }
            TraceabilityRequest::CreatePartId => self.create_part_id().await?,
            TraceabilityRequest::ReadPartSheet => self.handle_read().await?,
            TraceabilityRequest::SavePartSheets => self.handle_save().await?,
        }

        Ok(RESPONSE_SUCCESS)
    }

    /// Check if the request code value received at subscription time has already been
    /// answered before, by checking that the response code is not reset. This prevents
    /// handling twice a request answered before a reconnection, as requests are not
    /// idempotent. Reset request is idempotent, and thus is never considered answered.
    ///
    /// In case of error, the request is considered answered to stay on the safe side.
    #[instrument(skip_all)]
    pub(super) async fn is_initial_request_answered(&self, value: &DataValue) -> bool {
        if let Some(TraceabilityRequest::Reset) = value
            .clone()
            .try_ua_value_as()
            .ok()
            .and_then(TraceabilityRequest::from_repr)
        {
            return false;
        }

        let read_value_ids = &[ReadValueId::new_value(self.state.protocol.response.clone())];
        let values = match self
            .session
            .read(read_value_ids, TimestampsToReturn::Neither, 0.0)
            .await
        {
            Ok(values) => values,
            Err(err) => {
                warn!(msg = "error reading response code, ignoring initial request", %err);
                return true;
            }
        };
        let [response_value]: [DataValue; _] = match values.try_into() {
            Ok(v) => v,
            Err(err) => {
                warn!(
                    msg = "invalid read response length, ignoring initial request",
                    expected = 1,
                    got = err.len()
                );
                return true;
            }
        };
        match response_value.try_ua_value_as::<u8>() {
            Ok(code) => code != RESPONSE_RESET,
            Err(err) => {
                warn!(msg = "invalid response code value, ignoring initial request", %err);
                true
            }
        }
    }
}
