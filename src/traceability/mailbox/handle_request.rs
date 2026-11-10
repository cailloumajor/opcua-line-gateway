use std::mem;

use opcua::types::DataValue;
use strum::FromRepr;
use thiserror::Error;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};

use super::create_part_id::CreatePartIdError;
use super::read_part_sheet::HandleReadError;
use super::save_part_sheets::HandleSaveError;
use super::{MailboxContext, TraceabilityMailbox};

/// The response code for reset state.
const RESPONSE_RESET: u8 = 0;
/// The response code for success state.
const RESPONSE_SUCCESS: u8 = 1;

/// Traceability request code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub(crate) enum TraceabilityRequest {
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
pub(crate) enum HandleRequestError {
    #[error("invalid request value, cause: {0}")]
    ValueError(TryFromOpcUaValueError),
    #[error("unknown request value: {0}")]
    UnknownValue(u8),
    #[error("setting request {0:?} before start is forbidden")]
    InitialRequest(TraceabilityRequest),
    #[error("error creating the part ID")]
    CreatePartId(#[from] CreatePartIdError),
    #[error("error handling read request")]
    HandleRead(#[from] HandleReadError),
    #[error("error handling save request")]
    HandleSave(#[from] HandleSaveError),
}

impl HandleRequestError {
    /// Convert a request handling error to a traceability response code. This is
    /// intended to be used to generate a response code to write to the OPC-UA
    /// server in case of failure.
    pub(crate) fn to_response_code(&self) -> u8 {
        match self {
            Self::ValueError(_) => 91,
            Self::UnknownValue(_) => 92,
            Self::InitialRequest(_) => 93,

            // Group the two following errors in the same error code.
            Self::CreatePartId(CreatePartIdError::ReadVariables(_)) => 12,
            Self::CreatePartId(CreatePartIdError::ReadLength(_, _)) => 12,
            Self::CreatePartId(CreatePartIdError::PartRefValue(_)) => 13,
            Self::CreatePartId(CreatePartIdError::BatchValue(_)) => 14,
            // Group the two following errors in the same error code.
            Self::CreatePartId(CreatePartIdError::NextSerial(_)) => 15,
            Self::CreatePartId(CreatePartIdError::NextSerialTask(_)) => 15,
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

impl TraceabilityMailbox<MailboxContext> {
    /// Handle a request code from the OPC-UA server. Upon success, return the response code
    /// that must be written to the server.
    #[instrument(err, skip_all)]
    pub(crate) async fn handle_request(
        &mut self,
        value: DataValue,
    ) -> Result<u8, HandleRequestError> {
        let is_initial = mem::take(&mut self.state.request_is_initial);

        let request_code = value
            .try_ua_value_as()
            .map_err(HandleRequestError::ValueError)?;
        let req = TraceabilityRequest::from_repr(request_code)
            .ok_or(HandleRequestError::UnknownValue(request_code))?;

        if is_initial && req != TraceabilityRequest::Reset {
            return Err(HandleRequestError::InitialRequest(req));
        }

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
}
