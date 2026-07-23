use std::time::Duration;

use opcua::types::{NodeId, StatusCode};
use thiserror::Error;
use tokio::task::JoinError;

use crate::opcua::data_value::TryFromDataValueError;

use super::part_id::PartIdentifierError;

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
    #[error("error getting request value, cause: {0}")]
    ValueError(TryFromDataValueError),
    #[error("unknown request value: {0}")]
    UnknownValue(u8),
    #[error("error creating the part ID")]
    CreatePartId(#[from] CreatePartIdError),
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
            Self::CreatePartId(CreatePartIdError::NextSerialTask(_)) => 15,
            Self::CreatePartId(CreatePartIdError::NextSerial(_)) => 16,
            Self::CreatePartId(CreatePartIdError::PartIdentifier(_)) => 17,
            Self::CreatePartId(CreatePartIdError::WritePartId(_)) => 18,
        }
    }
}

/// Errors that can occur during reading from the server.
#[derive(Debug, Error)]
pub(super) enum ReadError {
    #[error("error getting traceability namespace index")]
    GetNamespaceIndex(#[source] opcua::types::Error),
    #[error("read request error")]
    ReadRequest(#[source] opcua::types::Error),
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
pub(super) enum CreatePartIdError {
    #[error("part ID creation is not configured for this server")]
    NotConfigured,
    #[error("error reading required variables")]
    ReadVariables(#[source] ReadError),
    #[error("invalid raw part reference value, cause: {0}")]
    PartRefValue(TryFromDataValueError),
    #[error("invalid raw batch value, cause: {0}")]
    BatchValue(TryFromDataValueError),
    #[error("error joining the next_serial blocking task")]
    NextSerialTask(#[source] JoinError),
    #[error("error getting next serial number from cache")]
    NextSerial(#[source] redb::Error),
    #[error("error generating the part identifier")]
    PartIdentifier(#[source] PartIdentifierError),
    #[error("error writing the part ID")]
    WritePartId(#[source] WriteError),
}
