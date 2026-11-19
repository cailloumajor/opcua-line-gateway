use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clickhouse::Client;
use futures_util::StreamExt;
use opcua_line_gateway_config::TraceabilityDatabaseConfig;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::wrappers::IntervalStream;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, instrument};

use crate::traceability::cache::QueueTable;

use super::TraceabilityCache;

/// Errors that can occur during draining the part sheet queues.
#[derive(Debug, Error)]
enum DrainQueuesError {
    #[error("blocking task to get part sheet batch failed: {0}")]
    GetBatchTask(JoinError),
    #[error("error getting part sheet batch from queue: {0}")]
    GetBatch(redb::Error),
    #[error("error inserting part sheets batch: {0}")]
    Insert(clickhouse::error::Error),
    #[error("blocking task to remove part sheet queued entries failed: {0}")]
    RemoveQueuedTask(JoinError),
    #[error("error removing part sheet queued entries: {0}")]
    RemoveQueued(redb::Error),
}

/// Database for traceability data archiving.
pub(crate) struct TraceabilityDatabase {
    /// ClickHouse client.
    client: Client,
    /// A shareable handle to the traceability cache.
    cache: Arc<TraceabilityCache>,
    /// General_part_sheet_table.
    general_part_sheet_table: String,
    /// Operation part sheet table.
    operation_part_sheet_table: String,
}

impl TraceabilityDatabase {
    /// Create a new [`TraceabilityDatabase`], provided ClickHouse client configuration,
    /// credentials and traceability cache.
    ///
    /// # Errors
    ///
    /// An error is returned if reading the password from the configured file fails.
    pub(crate) fn new(
        config: &TraceabilityDatabaseConfig,
        password: SecretString,
        cache: Arc<TraceabilityCache>,
    ) -> Self {
        let client = Client::default()
            .with_url(config.url.as_str())
            .with_user(&config.user)
            .with_password(password.expose_secret())
            .with_database(&config.default_database);

        let general_part_sheet_table = config.general_part_sheet_table.clone();
        let operation_part_sheet_table = config.operation_part_sheet_table.clone();

        Self {
            client,
            cache,
            general_part_sheet_table,
            operation_part_sheet_table,
        }
    }

    /// Start a task to periodically drain the part sheets to the database, according
    /// to provided period. Runs forever until provided shutdown is triggered.
    ///
    /// Performs an initial drain of each queue before spawning the task, returning
    /// an error if it fails.
    pub(crate) async fn drain_part_sheets_task(
        self,
        period: Duration,
        shutdown: CancellationToken,
    ) -> anyhow::Result<JoinHandle<()>> {
        // Drain each queue and insert in database, even with no data, to test
        // the connection.
        self.drain_part_sheet_queue(QueueTable::General, false)
            .await
            .context("Initial drain of general part sheet queue failed")?;
        self.drain_part_sheet_queue(QueueTable::Operation, false)
            .await
            .context("Initial drain of operation part sheet queue failed")?;

        let task = tokio::spawn(
            async move {
                info!(msg = "part sheets draining task started");

                // Build a stream producing periodically until shutdown is triggered.
                let mut interval = interval(period);
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                let stream = IntervalStream::new(interval).take_until(shutdown.cancelled());
                let mut pinned_stream = pin!(stream);

                while pinned_stream.next().await.is_some() {
                    debug!(msg = "draining part sheets rows from queues to the database");

                    let (general_result, operation_result) = tokio::join!(
                        self.drain_part_sheet_queue(QueueTable::General, true),
                        self.drain_part_sheet_queue(QueueTable::Operation, true)
                    );

                    if general_result.is_err() {
                        error!(msg = "error draining general part sheet queue");
                    }
                    if operation_result.is_err() {
                        error!(msg = "error draining operation part sheet queue");
                    }
                }

                info!(msg = "part sheets draining task terminated");
            }
            .instrument(info_span!(parent: None, "part_sheets_drain_task")),
        );

        Ok(task)
    }

    /// Drain part sheet queues to the database, provided the table to act on and
    /// a flag allowing to skip insertion if the queue is empty.
    #[instrument(err, skip_all, fields(queue = %queue_table))]
    async fn drain_part_sheet_queue(
        &self,
        queue_table: QueueTable,
        skip_if_empty: bool,
    ) -> Result<(), DrainQueuesError> {
        const SEND_TIMEOUT: Option<Duration> = Some(Duration::from_secs(2));
        const END_TIMEOUT: Option<Duration> = Some(Duration::from_secs(5));

        // Retrieve the part sheet queued rows.
        let sent_cache = Arc::clone(&self.cache);
        let get_batch_task =
            tokio::task::spawn_blocking(move || sent_cache.get_queue_batch(queue_table));
        let (keys, body) = get_batch_task
            .await
            .map_err(DrainQueuesError::GetBatchTask)?
            .map_err(DrainQueuesError::GetBatch)?;

        if skip_if_empty && keys.is_empty() {
            debug!(msg = "no row to drain");
            return Ok(());
        }

        // Insert the part sheet rows in database.
        let query = format!(
            "INSERT INTO {} FORMAT JSONEachRow",
            self.part_sheet_table(queue_table)
        );
        let mut inserter = self
            .client
            .insert_formatted_with(query)
            .with_timeouts(SEND_TIMEOUT, END_TIMEOUT);
        inserter
            .send(body.into())
            .await
            .map_err(DrainQueuesError::Insert)?;
        inserter.end().await.map_err(DrainQueuesError::Insert)?;

        info!(
            msg = "inserted drained rows in the database",
            count = keys.len()
        );

        // Remove the part sheet queue elements.
        let sent_cache = Arc::clone(&self.cache);
        let remove_task =
            tokio::task::spawn_blocking(move || sent_cache.remove_entries(queue_table, &keys));
        remove_task
            .await
            .map_err(DrainQueuesError::RemoveQueuedTask)?
            .map_err(DrainQueuesError::RemoveQueued)?;

        Ok(())
    }

    /// Get the database table corresponding to the provided queue table.
    fn part_sheet_table(&self, queue_table: QueueTable) -> &str {
        match queue_table {
            QueueTable::General => &self.general_part_sheet_table,
            QueueTable::Operation => &self.operation_part_sheet_table,
        }
    }
}
