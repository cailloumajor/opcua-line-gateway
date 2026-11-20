use std::sync::Arc;

use anyhow::Context as _;
use opcua_line_gateway_config::{Credentials, LineGatewayConfig};
use redb::Database;
use tokio_util::sync::CancellationToken;

use self::opcua::{create_client, sessions_manager};
pub use self::signals::spawn_signals_handler;
pub use self::time::TimeProvider;
use self::traceability::{TraceabilityCache, TraceabilityDatabase};

mod opcua;
mod signals;
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
        TraceabilityDatabase::from_config(db_config, credentials.database_password, db_cache)
            .context("Failed to initialize traceability database client")?;

    // Start part sheets draining task, failing if initial drain and insertion fails.
    let draining_task = traceability_database
        .drain_part_sheets_task(
            config.traceability.database.part_sheets_drain_period,
            shutdown.clone(),
        )
        .await
        .context("Failed to start draining task")?;

    // Create OPC-UA client.
    let client = create_client(&config).context("Failed to create OPC-UA client")?;

    // Start the sessions manager, acting as the main program loop.
    sessions_manager(
        client.into(),
        config.machines,
        credentials.opc_ua,
        config.traceability,
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
