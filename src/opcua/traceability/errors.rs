use std::time::Duration;

use opcua::types::{NodeId, StatusCode};
use thiserror::Error;

use crate::opcua::data_value::TryFromDataValueError;

use super::protocol::TraceabilityResponse;

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {}

/// Errors that can be encountered during traceability handler installation.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInstallError {
    #[error("error creating subscription: {0}")]
    CreateSubscription(#[source] opcua::types::Error),
    #[error("server raised publishing interval (requested {0:?}, got {1:?})")]
    PublishIntervalRaised(Duration, Duration),
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("error creating monitored items: {0}")]
    CreateMonitoredItems(#[source] opcua::types::Error),
    #[error("error on monitored item `{0}`: {1}")]
    MonitoredItem(NodeId, StatusCode),
}

/// Errors that can be encountered during request handling.
#[derive(Debug, Error)]
pub(super) enum HandleRequestError {
    #[error("error getting request value")]
    ValueError(#[from] TryFromDataValueError),
    #[error("unknown request value: {0}")]
    UnknownValue(u8),
    #[error("error writing response code")]
    WriteResponse(#[from] WriteError),
    #[error("error creating the part ID")]
    CreatePartId(#[from] CreatePartIdError),
}

impl HandleRequestError {
    /// Convert a request handling error to a traceability response code. This is intended
    /// to be used to generate a response code to write to the OPC-UA server in case
    /// of failure. Return `None` if not applicable.
    pub(super) fn to_response_code(&self) -> Option<TraceabilityResponse> {
        match self {
            Self::ValueError(_) => Some(TraceabilityResponse::RequestGetValueError),
            Self::UnknownValue(_) => Some(TraceabilityResponse::RequestUnknownValue),
            Self::WriteResponse(_) => None,
            Self::CreatePartId(_) => todo!(),
        }
    }
}

/// Errors that can be encountered during writing to the server.
#[derive(Debug, Error)]
pub(super) enum WriteError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("write request error")]
    WriteRequest(#[source] opcua::types::Error),
    #[error("write operation error: {0}")]
    WriteStatus(StatusCode),
}

/// Errors that can occur during part ID creation.
#[derive(Debug, Error)]
pub(super) enum CreatePartIdError {}
