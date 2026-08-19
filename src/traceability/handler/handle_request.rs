use opcua::types::DataValue;
use thiserror::Error;
use tracing::{info, instrument};

use crate::opcua::{DataValueExt, TryFromOpcUaValueError};
use crate::traceability::protocol::{RESPONSE_RESET, RESPONSE_SUCCESS, TraceabilityRequest};

use super::create_part_id::CreatePartIdError;
use super::read_part_sheet::ReadPartSheetError;
use super::save_part_sheets::SavePartSheetsError;
use super::{Initialized, TraceabilityHandler};

/// Errors that can be encountered during request handling.
#[derive(Debug, Error)]
pub(super) enum HandleRequestError {
    #[error("error getting request value, cause: {0}")]
    ValueError(TryFromOpcUaValueError),
    #[error("unknown request value: {0}")]
    UnknownValue(u8),
    #[error("error creating the part ID")]
    CreatePartId(#[from] CreatePartIdError),
    #[error("error reading the general part sheet")]
    ReadPartSheet(#[from] ReadPartSheetError),
    #[error("error saving part sheets")]
    SavePartSheets(#[from] SavePartSheetsError),
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

            Self::ReadPartSheet(ReadPartSheetError::ReadPartId(_)) => 21,
            Self::ReadPartSheet(ReadPartSheetError::PartIdValue(_)) => 22,
            Self::ReadPartSheet(ReadPartSheetError::CacheGet(_)) => 23,
            Self::ReadPartSheet(ReadPartSheetError::CacheGetTask(_)) => 24,
            Self::ReadPartSheet(ReadPartSheetError::CacheMissing(_)) => 25,
            Self::ReadPartSheet(ReadPartSheetError::WritePartSheet(_)) => 26,

            Self::SavePartSheets(SavePartSheetsError::ReadGeneralPartSheet(_)) => 31,
            Self::SavePartSheets(SavePartSheetsError::GeneralPartSheetLength(_, _)) => 32,
            Self::SavePartSheets(SavePartSheetsError::GeneralPartSheetValue(_, _)) => 33,
            Self::SavePartSheets(SavePartSheetsError::PartIdValue(_)) => 34,
            Self::SavePartSheets(SavePartSheetsError::CacheInsert(_)) => 35,
            Self::SavePartSheets(SavePartSheetsError::CacheInsertTask(_)) => 36,
        }
    }
}

impl TraceabilityHandler<Initialized> {
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
            TraceabilityRequest::ReadPartSheet => self.read_part_sheet().await?,
            TraceabilityRequest::SavePartSheets => self.save_part_sheets().await?,
        }

        Ok(RESPONSE_SUCCESS)
    }
}
