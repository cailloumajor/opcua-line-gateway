use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use opcua_line_gateway_config::{Credentials, LineGatewayConfig};
use redb::Database;
use tokio_util::sync::CancellationToken;
use tracing::info;

use self::opcua::{create_client, sessions_manager};
pub use self::time::TimeProvider;
use self::traceability::{TraceabilityCache, TraceabilityDatabase};

mod opcua;
mod time;
mod traceability;

/// OPC-UA line gateway main function.
pub async fn run_main(
    config: LineGatewayConfig,
    credentials: Credentials,
    time_provider: TimeProvider,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    // Create or open the traceability cache database (redb).
    let traceability_cache_db = Database::create(&config.traceability.redb_file)
        .context("Failed to open traceability cache database file")?;
    let traceability_cache = Arc::new(TraceabilityCache::new(
        traceability_cache_db,
        config.traceability.queues_not_draining_threshold,
    ));

    // Create traceability database client and fetch the general part sheet columns.
    let db_config = &config.traceability.database;
    let db_cache = Arc::clone(&traceability_cache);
    let traceability_database =
        TraceabilityDatabase::new(db_config, credentials.database, db_cache);

    // Start part sheets draining task.
    let (first_drain_status_rx, draining_task) = traceability_database.drain_part_sheets_task(
        config.traceability.database.part_sheets_drain_period,
        shutdown.clone(),
    );

    // Wait for the result of the first draining task run, and exit with error
    // if unsuccessful.
    let first_drain_successful = first_drain_status_rx
        .await
        .context("Failed to get the first draining task run status")?;
    if first_drain_successful {
        info!(msg = "first draining task run was successful");
    } else {
        return Err(anyhow!("First run of draining task was not successful"));
    }

    // Create OPC-UA client.
    let client = create_client(&config).context("Failed to create OPC-UA client")?;

    // Start the sessions manager, acting as the main program loop.
    sessions_manager(
        client.into(),
        config.machines,
        credentials.opc_ua,
        config.traceability.opc_ua,
        shutdown,
        traceability_cache,
        time_provider,
    )
    .await;

    draining_task
        .await
        .context("failed to join draining task")?;

    Ok(())
}
