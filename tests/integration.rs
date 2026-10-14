use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use opcua::types::Variant;
use opcua_line_gateway::TimeProvider;
use tempfile::TempDir;
use tick::SimpleClock;
use tracing_subscriber::EnvFilter;

use self::utils::database::init_database;
use self::utils::gateway::{TestGateway, create_gateway_pki};
use self::utils::opcua_server::{OpcUaTestServer, common_nid};

mod utils;

/// Initialize logging and directories for an integration test.
///
/// Returns the temporary directory for the test.
fn init() -> TempDir {
    // Install a global subscriber, as a thread-local one would not catch logs
    // from tasks running on other worker threads. This may have already been
    // done by another test.
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("creating temporary directory should not fail")
}

/// A request set before the gateway subscribes, and not answered yet, must be handled.
#[tokio::test]
#[ignore = "requires the database"]
async fn initial_request_pending() {
    let temp_dir = init();
    create_gateway_pki(temp_dir.path());
    let (db_config, db_creds) = init_database();

    let operation_nodes = vec![("DummyVar", false.into())];
    let server = OpcUaTestServer::start(
        temp_dir.path(),
        Some("AB".parse().unwrap()),
        operation_nodes,
    )
    .await;

    server.write_common_node(common_nid::RAW_PART_REF, "1-498GR13");
    server.write_common_node(common_nid::RAW_BATCH, "AA");
    server.write_common_node(common_nid::REQUEST, 1u8);

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    let response = server.wait_response(|code| code != 0).await;
    assert_eq!(response, 1);

    let part_id = server.read_common_node(common_nid::PART_ID);
    assert_ne!(part_id, Variant::from(""));

    server.traceability_reset().await;

    gateway.stop().await;
    server.stop().await;
}

/// A request set before the gateway subscribes, and already answered, must be ignored.
#[tokio::test]
#[ignore = "requires the database"]
async fn initial_request_answered() {
    let temp_dir = init();
    create_gateway_pki(temp_dir.path());
    let (db_config, db_creds) = init_database();

    let operation_nodes = vec![("DummyVar", false.into())];
    let server = OpcUaTestServer::start(temp_dir.path(), None, operation_nodes).await;

    server.write_common_node(common_nid::RAW_PART_REF, "1-498GR13");
    server.write_common_node(common_nid::RAW_BATCH, "AA");
    server.write_common_node(common_nid::REQUEST, 2u8);
    server.write_common_node(common_nid::RESPONSE, 1u8);

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    // Leave time for the initial value to be notified to the gateway, otherwise
    // it could be merged with the reset below, making this test pass vacuously.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Requests are handled in order, so once the reset is answered, the initial
    // request would have been handled if it was not ignored.
    server.traceability_reset().await;

    let part_id = server.read_common_node(common_nid::PART_ID);
    assert_eq!(part_id, Variant::from(""));

    gateway.stop().await;
    server.stop().await;
}

/// A reset request set before the gateway subscribes must reset the response code.
#[tokio::test]
#[ignore = "requires the database"]
async fn initial_request_reset() {
    let temp_dir = init();
    create_gateway_pki(temp_dir.path());
    let (db_config, db_creds) = init_database();

    let operation_nodes = vec![("DummyVar", false.into())];
    let server = OpcUaTestServer::start(
        temp_dir.path(),
        Some("AB".parse().unwrap()),
        operation_nodes,
    )
    .await;

    server.write_common_node(common_nid::RESPONSE, 1u8);

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    server.wait_response(|code| code == 0).await;

    gateway.stop().await;
    server.stop().await;
}

#[tokio::test]
#[ignore = "requires the database"]
async fn create_part_id() {
    let temp_dir = init();
    create_gateway_pki(temp_dir.path());
    let (db_config, db_creds) = init_database();

    let operation_nodes = vec![("DummyVar", false.into())];
    let server = OpcUaTestServer::start(
        temp_dir.path(),
        Some("AB".parse().unwrap()),
        operation_nodes,
    )
    .await;

    let fake_time = "3044-11-12 00:00:00Z".parse::<Timestamp>().unwrap();
    let time_provider = TimeProvider::new(SimpleClock::new_frozen_at(fake_time), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    server.write_common_node(common_nid::RAW_PART_REF, "1-498GR13");
    server.write_common_node(common_nid::RAW_BATCH, "AA");

    // Do the request to create a part sheet identifier.
    let response = server.traceability_request(1).await;

    assert_eq!(response, 1);

    let part_id = server.read_common_node(common_nid::PART_ID);
    assert_eq!(part_id, Variant::from("010498013AAAB4431700001"));

    gateway.stop().await;
    server.stop().await;
}
