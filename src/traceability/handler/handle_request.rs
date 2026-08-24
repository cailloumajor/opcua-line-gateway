use opcua::types::DataValue;
use thiserror::Error;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};
use crate::traceability::protocol::{RESPONSE_RESET, RESPONSE_SUCCESS, TraceabilityRequest};

use super::create_part_id::CreatePartIdError;
use super::read_part_sheet::HandleReadError;
use super::save_part_sheets::HandleSaveError;
use super::{TraceabilityContext, TraceabilityHandler};

/// Errors that can be encountered during request handling.
#[derive(Debug, Error)]
pub(super) enum HandleRequestError {
    #[error("error getting request value, cause: {0}")]
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
    /// of failure. Return `None` if not applicable.
    pub(super) fn to_response_code(&self) -> u8 {
        match self {
            Self::ValueError(_) => 91,
            Self::UnknownValue(_) => 92,

            Self::CreatePartId(CreatePartIdError::NotConfigured) => 11,
            Self::CreatePartId(CreatePartIdError::ReadVariables(_)) => 12,
            Self::CreatePartId(CreatePartIdError::PartRefValue(_)) => 13,
            Self::CreatePartId(CreatePartIdError::BatchValue(_)) => 14,
            Self::CreatePartId(CreatePartIdError::NextSerial(_)) => 15,
            Self::CreatePartId(CreatePartIdError::PartIdentifier(_)) => 16,
            Self::CreatePartId(CreatePartIdError::WritePartId(_)) => 17,

            Self::HandleRead(HandleReadError::ReadPartId(_)) => 21,
            Self::HandleRead(HandleReadError::PartIdValue(_)) => 22,
            Self::HandleRead(HandleReadError::CacheGet(_)) => 23,
            Self::HandleRead(HandleReadError::CacheGetTask(_)) => 24,
            Self::HandleRead(HandleReadError::CacheMissing(_)) => 25,
            Self::HandleRead(HandleReadError::WritePartSheet(_)) => 26,

            Self::HandleSave(HandleSaveError::ReadGeneralPartSheet(_)) => 31,
            Self::HandleSave(HandleSaveError::GeneralPartSheetLength(_, _)) => 32,
            Self::HandleSave(HandleSaveError::GeneralPartSheetValue(_, _)) => 33,
            Self::HandleSave(HandleSaveError::PartIdValue(_)) => 34,
            Self::HandleSave(HandleSaveError::InvalidPartId(_)) => 35,
            Self::HandleSave(HandleSaveError::CacheSave(_)) => 36,
            Self::HandleSave(HandleSaveError::CacheSaveTask(_)) => 37,
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
}
