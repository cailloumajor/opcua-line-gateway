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

/// Common initialization for integration tests.
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

    let temp_dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("creating temporary directory should not fail");

    create_gateway_pki(temp_dir.path());

    temp_dir
}

/// A request set before the gateway subscribes, and not answered yet, must be handled.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn initial_request_pending() {
    let temp_dir = init();
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
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn initial_request_answered() {
    let temp_dir = init();
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
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn initial_request_reset() {
    let temp_dir = init();
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
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn create_part_id() {
    let temp_dir = init();
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

/// A general part sheet saved from a server must be read back identically from another one.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn general_part_sheet_roundtrip() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let operation_nodes = vec![("DummyVar", false.into())];
    let server_a = OpcUaTestServer::start(temp_dir.path(), None, operation_nodes.clone()).await;
    let server_b = OpcUaTestServer::start(temp_dir.path(), None, operation_nodes).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server_a, &server_b],
    )
    .await;

    let part_id = Variant::from("010498013AAAB4431700001");

    // Fill the general part sheet nodes of the first server with non-default values.
    let general_part_sheet: [(_, Variant); _] = [
        (common_nid::PART_ID, part_id.clone()),
        (common_nid::RAW_PART_REF, "1-498GR13".into()),
        (common_nid::RAW_BATCH, "AA".into()),
        (1004, "1-498GR13-F".into()),
        (1005, true.into()),
        (1006, true.into()),
        (1007, vec![true, false, true, false, true].into()),
        (1008, vec![false, true, false, true, false].into()),
        (1009, "Alice".into()),
        (1010, "Bob".into()),
        (1011, "PRG-42".into()),
        (1012, vec![true, true, false, false, true].into()),
        (1013, vec![false, false, true, true, false].into()),
        (1014, vec![true, false, false, false, true].into()),
        (1015, true.into()),
        (1016, true.into()),
        (1017, true.into()),
        (1018, true.into()),
        (1019, true.into()),
        (1020, "Scratch".into()),
        (1021, "Charlie".into()),
    ];
    for (nid, val) in general_part_sheet.clone() {
        server_a.write_common_node(nid, val);
    }

    // Save the part sheets from the first server.
    let response = server_a.traceability_request(3).await;
    assert_eq!(response, 1);

    // Read the general part sheet from the second server.
    server_b.write_common_node(common_nid::PART_ID, part_id);
    let response = server_b.traceability_request(2).await;
    assert_eq!(response, 1);

    for (nid, val) in general_part_sheet {
        assert_eq!(server_b.read_common_node(nid), val, "node {nid} value");
    }

    gateway.stop().await;
    server_a.stop().await;
    server_b.stop().await;
}
