use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use opcua::client::DataChangeCallback;
use opcua::types::{
    MonitoredItemCreateRequest, MonitoringMode, MonitoringParameters, NodeId, StatusCode,
    TimestampsToReturn, WriteValue,
};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, info_span, instrument, warn};

use crate::opcua::SessionExt;

use super::{HandlerContext, TraceabilityHandler};

/// The duration between heartbeat changes.
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

/// Errors that can be encountered during traceability handler installation.
#[derive(Debug, Error)]
pub(crate) enum TraceabilityInstallError {
    #[error("error creating subscription: {0}")]
    CreateSubscription(#[source] opcua::types::Error),
    #[error("subscription not found for ID {0}")]
    GetSubscription(u32),
    #[error("server raised publishing interval (requested {0:?}, got {1:?})")]
    PublishIntervalRaised(Duration, Duration),
    #[error("error creating monitored items: {0}")]
    CreateMonitoredItems(#[source] opcua::types::Error),
    #[error("error on monitored item `{0}`: {1}")]
    MonitoredItem(NodeId, StatusCode),
}

impl TraceabilityHandler<HandlerContext> {
    /// Install this handler to allow it to handle requests, provided a shutdown
    /// token and the subscription publish interval to request to the server.
    ///
    /// Returns the collection of traceability tasks.
    #[instrument(name = "traceability_install", err, skip_all)]
    pub(crate) async fn install(
        mut self,
        shutdown: CancellationToken,
        publish_interval: Duration,
    ) -> Result<JoinSet<()>, TraceabilityInstallError> {
        // Create a channel to pass data change notifications to the traceability handler.
        let (tx, rx) = mpsc::unbounded_channel();

        let data_change_callback = DataChangeCallback::new(move |value, monitored_item| {
            if tx.send((monitored_item.client_handle(), value)).is_err() {
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
            .ok_or_else(|| TraceabilityInstallError::GetSubscription(subscription_id))?
            .publishing_interval();

        if revised_publishing_interval > publish_interval {
            // Optimistic attempt to delete the subscription.
            let _ = self.session.delete_subscription(subscription_id).await;
            return Err(TraceabilityInstallError::PublishIntervalRaised(
                publish_interval,
                revised_publishing_interval,
            ));
        }

        // Create the monitored item. Given that we only have one item, we use sane
        // defaults, including not attributing client ID to monitored item.
        let monitored_items = self
            .state
            .mailboxes
            .iter()
            .zip(1..)
            .map(|(mailbox, client_handle)| MonitoredItemCreateRequest {
                item_to_monitor: mailbox.request_node().into(),
                monitoring_mode: MonitoringMode::Reporting,
                requested_parameters: MonitoringParameters {
                    client_handle,
                    ..Default::default()
                },
            })
            .collect();
        let created = self
            .session
            .create_monitored_items(subscription_id, TimestampsToReturn::Source, monitored_items)
            .await
            .map_err(TraceabilityInstallError::CreateMonitoredItems)?;

        // Check if created items are healthy.
        if let Some(failed_item) = created
            .into_iter()
            .find(|item| !item.result.status_code.is_good())
        {
            let nid = failed_item.item_to_monitor.node_id;
            let code = failed_item.result.status_code;

            return Err(TraceabilityInstallError::MonitoredItem(nid, code));
        }

        // Create a collection of tasks.
        let mut tasks = JoinSet::new();

        // Spawn heartbeat task.
        let heartbeat_shutdown = shutdown.clone();
        let server_id = self.machine_id.clone();
        let sent_session = Arc::clone(&self.session);
        let heartbeat_node = self.state.heartbeat.clone();
        tasks.spawn(
            async move {
                info!(msg = "heartbeat handler started");

                // Heartbeat write value.
                let mut hb_value = true;

                let mut hb_interval = interval(HEARTBEAT_INTERVAL);
                hb_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                let stream =
                    IntervalStream::new(hb_interval).take_until(heartbeat_shutdown.cancelled());
                let mut pinned_stream = pin!(stream);
                while pinned_stream.next().await.is_some() {
                    let write_value =
                        WriteValue::value_attr(heartbeat_node.clone(), hb_value.into());
                    hb_value = !hb_value;
                    // Ignore the result, as error logging is handled by the function
                    // `instrument` attribute.
                    let _ = sent_session.write_checked(&[write_value]).await;
                }

                info!(msg = "heartbeat handler terminated");
            }
            .instrument(info_span!(parent: None, "heartbeat_handler", %server_id)),
        );

        // Spawn traceability request handling task.
        let machine_id = self.machine_id.clone();
        let sent_session = Arc::clone(&self.session);
        tasks.spawn(
            async move {
                info!(msg = "traceability handler started");

                // Make a stream out of requests receiver, with graceful shutdown.
                let stream = UnboundedReceiverStream::new(rx).take_until(shutdown.cancelled());
                let mut pinned_stream = pin!(stream);
                while let Some((client_handle, request_value)) = pinned_stream.next().await {
                    let Some(target_mailbox) = client_handle
                        .checked_sub(1)
                        .and_then(|i| usize::try_from(i).ok())
                        .and_then(|index| self.state.mailboxes.get_mut(index))
                    else {
                        error!(
                            msg = "invalid client handle as mailbox index",
                            client_handle
                        );
                        continue;
                    };
                    let response_value = target_mailbox
                        .handle_request(request_value)
                        .await
                        .unwrap_or_else(|e| e.to_response_code());
                    let write_value = WriteValue::value_attr(
                        target_mailbox.response_node().clone(),
                        response_value.into(),
                    );
                    // Ignore the result, as error logging is handled by the function
                    // `instrument` attribute.
                    let _ = sent_session.write_checked(&[write_value]).await;
                }

                info!(msg = "traceability handler terminated");
            }
            .instrument(info_span!(parent:None, "traceability_handler", %machine_id)),
        );

        Ok(tasks)
    }
}
