use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use jiff::Zoned;
use opcua::client::{DataChangeCallback, Session};
use opcua::types::{DataValue, IntoVariant, NodeId, ReadValueId, TimestampsToReturn, WriteValue};
use opcua_line_gateway_config::{AsciiText, TraceabilityConfig};
use redb::Database;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, info_span, instrument, warn};

use crate::opcua::data_value::DataValueExt;

use self::cache::TraceabilityCache;
use self::errors::{CreatePartIdError, HandleRequestError, ReadError, WriteError};
pub(super) use self::errors::{TraceabilityInitializeError, TraceabilityInstallError};
use self::protocol::{TraceabilityRequest, TraceabilityResponse};

mod cache;
mod errors;
mod part_id;
mod protocol;

/// The duration between heartbeat changes.
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

/// The initial state of the traceability handler.
pub(super) struct InitialState;

/// The traceability handler state after initialization.
#[derive(Clone)]
pub(super) struct Initialized {}

/// Manages traceability for an OPC-UA session.
#[derive(Clone)]
pub(super) struct TraceabilityHandler<S> {
    /// The ID of the server this handler works with.
    server_id: String,
    /// The configuration for this server.
    config: TraceabilityConfig,
    /// The OPC-UA session.
    session: Arc<Session>,
    /// The traceability cache.
    cache: TraceabilityCache,
    /// The state of this handler.
    state: S,
}

impl TraceabilityHandler<InitialState> {
    /// Create a new [`TraceabilityHandler`].
    pub(super) fn new(
        server_id: String,
        config: TraceabilityConfig,
        session: Arc<Session>,
        cache_db: Arc<Database>,
    ) -> Self {
        let cache = TraceabilityCache::new(cache_db);

        Self {
            server_id,
            config,
            session,
            cache,
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
            cache: self.cache,
            state,
        })
    }
}

impl TraceabilityHandler<Initialized> {
    /// Install this handler to allow it to handle requests. This mainly consists in
    /// subscribing to the request variable.
    ///
    /// Returns the collection of traceability tasks.
    #[instrument(name = "traceability_install", err, skip_all)]
    #[must_use = "the returned handle should be used"]
    pub(super) async fn install(
        self,
        shutdown: CancellationToken,
    ) -> Result<JoinSet<()>, TraceabilityInstallError> {
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

        // Create a collection of tasks.
        let mut tasks = JoinSet::new();

        // Spawn heartbeat task.
        let heartbeat_shutdown = shutdown.clone();
        let server_id = self.server_id.clone();
        let cloned_self = self.clone();
        tasks.spawn(
            async move {
                info!(msg = "heartbeat handler started");

                // Heartbeat value.
                let mut hb_value = false;

                let mut hb_interval = interval(HEARTBEAT_INTERVAL);
                hb_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                let stream = IntervalStream::new(hb_interval)
                    .map(|_| {
                        // Revert heartbeat value and return it.
                        hb_value ^= true;
                        hb_value
                    })
                    .take_until(heartbeat_shutdown.cancelled());
                let mut pinned_stream = pin!(stream);
                while let Some(value) = pinned_stream.next().await {
                    let _ = cloned_self
                        .write_value(self.config.heartbeat_node_id, value)
                        .await;
                }

                info!(msg = "heartbeat handler terminated");
            }
            .instrument(info_span!(parent: None, "heartbeat_handler", server_id)),
        );

        // Spawn traceability request handling task.
        let server_id = self.server_id.clone();
        tasks.spawn(
            async move {
                info!(msg = "traceability handler started");

                // Make a stream out of requests receiver, with graceful shutdown.
                // The first value produced is discarded, to prevent handling
                // a request code that would have been set before we started.
                let stream = UnboundedReceiverStream::new(rx)
                    .take_until(shutdown.cancelled())
                    .skip(1);
                let mut pinned_stream = pin!(stream);
                while let Some(request_value) = pinned_stream.next().await {
                    let result = self.handle_request(request_value).await;
                    if let Err(Some(response)) = result.map_err(|e| e.to_response_code()) {
                        // Ignore the result, as error logging is handled by the function
                        // `instrument` attribute.
                        let _ = self
                            .write_value(self.config.response_node_id, response)
                            .await;
                    }
                }

                info!(msg = "traceability handler terminated");
            }
            .instrument(info_span!(parent:None, "traceability_handler", server_id)),
        );

        Ok(tasks)
    }

    /// Handle a request code from the OPC-UA server.
    #[instrument(err, skip_all)]
    async fn handle_request(&self, value: DataValue) -> Result<(), HandleRequestError> {
        let request_code = value.try_as()?;
        let Some(req) = TraceabilityRequest::from_repr(request_code) else {
            return Err(HandleRequestError::UnknownValue(request_code));
        };

        match req {
            TraceabilityRequest::Reset => {
                self.write_value(self.config.response_node_id, TraceabilityResponse::Reset)
                    .await?
            }
            TraceabilityRequest::CreatePartId => self.create_part_id().await?,
            _ => todo!(),
        }

        Ok(())
    }

    /// Create the part ID by getting required data from the OPC-UA server and writing back the
    /// generated ID.
    #[instrument(err, skip_all)]
    async fn create_part_id(&self) -> Result<(), CreatePartIdError> {
        let config = self
            .config
            .part_identifier
            .as_ref()
            // Return an error if this instance has no part reference configuration.
            .ok_or(CreatePartIdError::NotConfigured)?;

        // Read and convert needed OPC-UA variables.
        let values = self
            .read_values(&[config.raw_part_ref_node_id, config.raw_batch_node_id])
            .await?;
        let [raw_part_ref_value, raw_batch_value] = values
            .try_into()
            .expect("read values vector should have the expected size");
        let raw_part_ref: &str = raw_part_ref_value
            .try_as()
            .map_err(CreatePartIdError::PartRefValue)?;
        let raw_batch: AsciiText<2> = raw_batch_value
            .try_as()
            .map_err(CreatePartIdError::BatchValue)?;

        let today = Zoned::now().date();

        todo!()
    }

    /// Read the values of nodes with provided identifiers.
    #[instrument(err, skip_all)]
    async fn read_values(&self, ids: &[u32]) -> Result<Vec<DataValue>, ReadError> {
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(ReadError::GetNamespaceIndex)?;
        let nodes_to_read = ids
            .iter()
            .map(|id| {
                let node_id = NodeId::new(ns_index, *id);
                ReadValueId::new_value(node_id)
            })
            .collect::<Vec<_>>();
        self.session
            .read(&nodes_to_read, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(ReadError::ReadRequest)
    }

    /// Write provided value to the provided node identifier.
    #[instrument(err, skip_all)]
    async fn write_value<T>(&self, id: u32, value: T) -> Result<(), WriteError>
    where
        T: IntoVariant,
    {
        let ns_index = self
            .session
            .get_namespace_index(&self.config.namespace_url)
            .await
            .map_err(WriteError::GetNamespaceIndex)?;
        let node_id = NodeId::new(ns_index, id);
        let write_value = WriteValue::value_attr(node_id, value.into());
        let results = self
            .session
            .write(&[write_value])
            .await
            .map_err(WriteError::WriteRequest)?;
        if let Some(status) = results.into_iter().find(|s| !s.is_good()) {
            return Err(WriteError::WriteStatus(status));
        }

        Ok(())
    }
}
