use thiserror::Error;
use tracing::instrument;

use super::{InitialState, TraceabilityHandler};

/// Errors that can be encountered during traceability handler initialization.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInitializeError {}

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(crate) struct Initialized {}

impl TraceabilityHandler<InitialState> {
    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
    pub(crate) async fn initialize(
        self,
    ) -> Result<TraceabilityHandler<Initialized>, TraceabilityInitializeError> {
        let state = Initialized {};

        Ok(TraceabilityHandler {
            server_id: self.server_id,
            config: self.config,
            session: self.session,
            cache: self.cache,
            state,
        })
    }
}
