use std::sync::Arc;

use futures_util::{StreamExt, pin_mut};
use opcua::client::{DataChangeCallback, Session};
use opcua::types::{DataValue, NodeId, TimestampsToReturn, Variant, WriteValue};
use opcua_line_gateway_config::TraceabilityConfig;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, info_span, instrument, warn};

use self::errors::{HandleRequestError, WriteResponseError};
pub(super) use self::errors::{TraceabilityInitializeError, TraceabilityInstallError};
use self::protocol::{TraceabilityRequest, TraceabilityResponse};

mod errors;
mod protocol;

/// The initial state of the traceability handler.
pub(super) struct InitialState;

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(super) struct Initialized {}

/// Manages traceability for an OPC-UA session.
#[derive(Clone)]
pub(super) struct TraceabilityHandler<S> {
    /// The ID of the server this handler works with.
    pub(super) server_id: String,
    /// The configuration for this server.
    pub(super) config: TraceabilityConfig,
    /// The OPC-UA session.
    pub(super) session: Arc<Session>,
    /// The state of this handler.
    state: S,
}

impl TraceabilityHandler<InitialState> {
    /// Create a new [`TraceabilityHandler`].
    pub(super) fn new(
        server_id: String,
        config: TraceabilityConfig,
        session: Arc<Session>,
    ) -> Self {
        Self {
            server_id,
            config,
            session,
            state: InitialState,
        }
    }

    /// Initialize the traceability handler. This involves interacting with the session.
    #[instrument(name = "traceability_initialize", err, skip_all)]
    pub(super) async fn initialize(
        self,
    ) -> Result<TraceabilityHandler<Initialized>, TraceabilityInitializeError> {
        let state = Initialized {};

        Ok(TraceabilityHandler {
            server_id: self.server_id,
            config: self.config,
            session: self.session,
            state,
        })
    }
}

impl TraceabilityHandler<Initialized> {
    /// Install this handler to allow it to handle requests. This mainly consists in
    /// subscribing to the request variable.
    ///
    /// Returns the handle to the request handling task.
    #[instrument(name = "traceability_install", err, skip_all)]
    #[must_use = "the returned handle should be used"]
    pub(super) async fn install(
        self,
        shutdown: CancellationToken,
    ) -> Result<JoinHandle<()>, TraceabilityInstallError> {
        let publish_interval = self.config.publish_interval;

        let (tx, rx) = mpsc::unbounded_channel();

        let data_change_callback = DataChangeCallback::new(move |value, _monitored_item| {
            if tx.send(value).is_err() {
                warn!(msg = "traceability channel closed, dropping notification");
            }
        });
        let subscription_id = self
            .session
            .create_subscription(publish_interval, 50, 10, 0, 0, true, data_change_callback)
            .await
            .map_err(TraceabilityInstallError::CreateSubscription)?;

        // Check that the requested publishing interval has not been raised by the server.
        let revised_publishing_interval = self
            .session
            .subscription_state()
            .lock()
            .get(subscription_id)
            .expect("getting successfully created subscription should not fail")
            .publishing_interval();

        if revised_publishing_interval > publish_interval {
            // Optimistic attempt to delete the subscription.
            let _ = self.session.delete_subscription(subscription_id).await;
            return Err(TraceabilityInstallError::PublishIntervalRaised(
                publish_interval,
                revised_publishing_interval,
            ));
        }

        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(TraceabilityInstallError::GetNamespaceIndex)?;
        let request_node_id = NodeId::new(ns_index, self.config.request_node_id);

        // Create the monitored item. Given that we only have one item, we use sane
        // defaults, including not attributing client ID to monitored item.
        let created = self
            .session
            .create_monitored_items(
                subscription_id,
                TimestampsToReturn::Source,
                vec![request_node_id.into()],
            )
            .await
            .map_err(TraceabilityInstallError::CreateMonitoredItems)?;

        // Check if created items are healthy.
        if let Some(failed_item) = created
            .into_iter()
            .find(|item| !item.result.status_code.is_good())
        {
            return Err(TraceabilityInstallError::MonitoredItem(
                failed_item.item_to_monitor.node_id,
                failed_item.result.status_code,
            ));
        }

        // Spawn traceability request handling task.
        let server_id = self.server_id.clone();
        let handle = tokio::spawn(
            async move {
                info!(msg = "traceability handler started");

                // Make a stream out of requests receiver, with graceful shutdown.
                // The first value produced is discarded, to prevent handling
                // a request code that would have been set before we started.
                let request_stream = UnboundedReceiverStream::new(rx)
                    .take_until(shutdown.cancelled())
                    .skip(1);
                pin_mut!(request_stream);
                while let Some(request_value) = request_stream.next().await {
                    let result = self.handle_request(request_value).await;
                    if let Err(Some(response)) = result.map_err(|e| e.to_response_code()) {
                        // Ignore the result, as error logging is handled by the function
                        // `instrument` attribute.
                        let _ = self.write_response(response).await;
                    }
                }

                info!(msg = "traceability handler terminated");
            }
            .instrument(info_span!(parent:None, "traceability_handler", server_id)),
        );

        Ok(handle)
    }

    /// Handle a request code from the OPC-UA server.
    #[instrument(err, skip_all)]
    async fn handle_request(&self, value: DataValue) -> Result<(), HandleRequestError> {
        let Some(variant) = value.value else {
            return Err(HandleRequestError::ValueMissing);
        };
        let Variant::Byte(request_code) = variant else {
            return Err(HandleRequestError::InvalidValue(variant));
        };
        let Some(req) = TraceabilityRequest::from_repr(request_code) else {
            return Err(HandleRequestError::UnknownValue(request_code));
        };

        match req {
            TraceabilityRequest::Reset => self.write_response(TraceabilityResponse::Reset).await?,
            _ => todo!(),
        }

        Ok(())
    }

    /// Reset the response code.
    #[instrument(err, skip_all)]
    async fn write_response(&self, code: TraceabilityResponse) -> Result<(), WriteResponseError> {
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(WriteResponseError::GetNamespaceIndex)?;
        let write_value = WriteValue::value_attr(
            NodeId::new(ns_index, self.config.response_node_id),
            code.into(),
        );
        let results = self
            .session
            .write(&[write_value])
            .await
            .map_err(WriteResponseError::WriteRequest)?;
        if let Some(status) = results.into_iter().find(|s| !s.is_good()) {
            return Err(WriteResponseError::WriteStatus(status));
        }

        Ok(())
    }
}
