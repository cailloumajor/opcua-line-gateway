//! Integration tests of the gateway, with OPC-UA servers exposing machines according
//! to the traceability information model (see `docs/information-model.md`).

use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use opcua::types::Variant;
use opcua_line_gateway::TimeProvider;
use serde_json::json;
use tempfile::TempDir;
use tick::SimpleClock;
use tracing_subscriber::EnvFilter;

use self::utils::database::{init_database, wait_archived_part_sheets};
use self::utils::gateway::{TestGateway, create_gateway_pki};
use self::utils::opcua_server::{OpcUaTestServer, TestMachine, general};

mod utils;

/// Traceability request codes.
mod request {
    pub const CREATE_PART_ID: u8 = 1;
    pub const LOAD_GENERAL_PART_SHEET: u8 = 2;
    pub const SAVE_PART_SHEETS: u8 = 3;
}

/// Response code of a successful request.
const RESPONSE_OK: u8 = 1;

/// Response code of a request set before the gateway subscribes.
const RESPONSE_INITIAL_REQUEST: u8 = 93;

/// A valid part identifier.
const PART_ID: &str = "010498013AAAB4431700001";

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

/// Machine with a single mailbox.
fn machine_op10() -> TestMachine {
    TestMachine::new(
        "OP10",
        &["TraceabilityMailboxOP10"],
        vec![("DummyVar", false.into())],
    )
}

/// Machine with two mailboxes, where parts enter and leave it.
fn machine_op20() -> TestMachine {
    TestMachine::new(
        "OP20",
        &["TraceabilityMailboxOP20In", "TraceabilityMailboxOP20Out"],
        vec![("DummyVar", false.into())],
    )
}

/// Machine with three mailboxes, where parts enter it, and leave it either as good
/// or scrapped parts.
fn machine_op30() -> TestMachine {
    TestMachine::new(
        "OP30",
        &[
            "TraceabilityMailboxOP30In",
            "TraceabilityMailboxOP30OkOut",
            "TraceabilityMailboxOP30ScrapOut",
        ],
        vec![("Conforming", false.into()), ("Operator", "".into())],
    )
}

/// Return a general part sheet filled with non-default values.
fn filled_general_part_sheet() -> Vec<(&'static str, Variant)> {
    vec![
        (general::PART_ID, PART_ID.into()),
        (general::RAW_PART_REF, "1-498GR13".into()),
        (general::RAW_BATCH, "AA".into()),
        ("RefePiecFini", "1-498GR13-F".into()),
        ("PiecAnnoChgtRefPiecFini", true.into()),
        ("PiecAnnoChgtLotMati", true.into()),
        (
            "SuivTravPiecParPost",
            vec![true, false, true, false, true].into(),
        ),
        (
            "SuivConfPiecParPost",
            vec![false, true, false, true, false].into(),
        ),
        ("QuiDeclNonConf", "Alice".into()),
        ("QuiDeclConf", "Bob".into()),
        ("ProgReceMach", "PRG-42".into()),
        (
            "SuivPostAvecPassTrav",
            vec![true, true, false, false, true].into(),
        ),
        (
            "SuivPrelPiecParPost",
            vec![false, false, true, true, false].into(),
        ),
        (
            "SuivRejePiecParPost",
            vec![true, false, false, false, true].into(),
        ),
        ("RefePiecFiniInco", true.into()),
        ("PiecTravInco", true.into()),
        ("PiecNonConfAmon", true.into()),
        ("LotMatiInco", true.into()),
        ("PiecRebu", true.into()),
        ("DeclRebuMoti", "Scratch".into()),
        ("DeclRebuQui", "Charlie".into()),
    ]
}

/// Assert that the general part sheet loaded in a mailbox matches the saved one,
/// except for the variables excluded from loading, which keep their default value.
fn assert_general_part_sheet_loaded(
    mailbox: &utils::opcua_server::TestMailbox<'_>,
    saved: &[(&str, Variant)],
) {
    for (name, value) in saved {
        let expected = if general::EXCLUDED_READ.contains(name) {
            Variant::from("")
        } else {
            value.clone()
        };
        assert_eq!(
            mailbox.read_general(name),
            expected,
            "variable {name} value"
        );
    }
}

/// A request set before the gateway subscribes must be refused, in the mailbox where it
/// was set only, and the requests following a reset must be handled.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn initial_request_refused() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op20()).await;
    let mailbox_in = server.mailbox("TraceabilityMailboxOP20In");
    let mailbox_out = server.mailbox("TraceabilityMailboxOP20Out");

    mailbox_out.write_general(general::RAW_PART_REF, "1-498GR13");
    mailbox_out.write_general(general::RAW_BATCH, "AA");
    mailbox_out.write_request(request::CREATE_PART_ID);

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    assert_eq!(mailbox_out.read_response(), RESPONSE_INITIAL_REQUEST);
    assert_eq!(
        mailbox_out.read_general(general::PART_ID),
        Variant::from("")
    );

    // The other mailbox must not have been touched.
    assert_eq!(mailbox_in.read_response(), 0);

    // Once reset, the mailbox must handle requests.
    mailbox_out.traceability_reset().await;
    let response = mailbox_out
        .traceability_request(request::CREATE_PART_ID)
        .await;
    assert_eq!(response, RESPONSE_OK);
    assert_ne!(
        mailbox_out.read_general(general::PART_ID),
        Variant::from("")
    );

    gateway.stop().await;
    server.stop().await;
}

/// A reset request set before the gateway subscribes must reset the response code,
/// in the mailbox where it was set.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn initial_request_reset() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op30()).await;
    let mailbox = server.mailbox("TraceabilityMailboxOP30ScrapOut");

    mailbox.write_response(RESPONSE_OK);

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    mailbox.wait_response(|code| code == 0).await;

    gateway.stop().await;
    server.stop().await;
}

#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn create_part_id() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op10()).await;
    let mailbox = server.mailbox("TraceabilityMailboxOP10");

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

    mailbox.write_general(general::RAW_PART_REF, "1-498GR13");
    mailbox.write_general(general::RAW_BATCH, "AA");

    // Do the request to create a part sheet identifier.
    let response = mailbox.traceability_request(request::CREATE_PART_ID).await;

    assert_eq!(response, RESPONSE_OK);

    let part_id = mailbox.read_general(general::PART_ID);
    assert_eq!(part_id, Variant::from(PART_ID));

    gateway.stop().await;
    server.stop().await;
}

/// The gateway must change the heartbeat of the machine regularly.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn heartbeat_changes() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op30()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    for _ in 0..2 {
        let initial = server.read_heartbeat();
        tokio::time::timeout(Duration::from_secs(2), async {
            while server.read_heartbeat() == initial {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the heartbeat should change");
    }

    gateway.stop().await;
    server.stop().await;
}

/// A request must be answered in the mailbox where it was made, and act on the part
/// sheets of this mailbox only.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn request_handled_in_its_mailbox() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op30()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    for mailbox in server.mailboxes() {
        mailbox.write_general(general::RAW_PART_REF, "1-498GR13");
        mailbox.write_general(general::RAW_BATCH, "AA");
    }

    let mailbox_ok_out = server.mailbox("TraceabilityMailboxOP30OkOut");
    mailbox_ok_out.write_request(request::CREATE_PART_ID);
    let response = mailbox_ok_out.wait_response(|code| code != 0).await;
    assert_eq!(response, RESPONSE_OK);
    assert_ne!(
        mailbox_ok_out.read_general(general::PART_ID),
        Variant::from("")
    );

    // The other mailboxes must not have been touched.
    for name in [
        "TraceabilityMailboxOP30In",
        "TraceabilityMailboxOP30ScrapOut",
    ] {
        let mailbox = server.mailbox(name);
        assert_eq!(mailbox.read_response(), 0, "response of mailbox {name}");
        assert_eq!(
            mailbox.read_general(general::PART_ID),
            Variant::from(""),
            "part identifier of mailbox {name}"
        );
    }

    mailbox_ok_out.traceability_reset().await;

    gateway.stop().await;
    server.stop().await;
}

/// Requests made at the same time in several mailboxes of a machine must all be
/// handled.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn simultaneous_requests_in_mailboxes() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op30()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server],
    )
    .await;

    let mailboxes = server.mailboxes();
    for mailbox in &mailboxes {
        mailbox.write_general(general::RAW_PART_REF, "1-498GR13");
        mailbox.write_general(general::RAW_BATCH, "AA");
    }
    for mailbox in &mailboxes {
        mailbox.write_request(request::CREATE_PART_ID);
    }

    let mut part_ids = Vec::new();
    for mailbox in &mailboxes {
        let response = mailbox.wait_response(|code| code != 0).await;
        assert_eq!(response, RESPONSE_OK);
        let part_id = mailbox.read_general(general::PART_ID);
        assert_ne!(part_id, Variant::from(""));
        part_ids.push(part_id);
    }
    for mailbox in &mailboxes {
        mailbox.traceability_reset().await;
    }

    // Each mailbox got its own part identifier.
    for (index, part_id) in part_ids.iter().enumerate() {
        assert!(
            !part_ids[index + 1..].contains(part_id),
            "part identifier {part_id:?} should be unique"
        );
    }

    gateway.stop().await;
    server.stop().await;
}

/// A general part sheet saved from a mailbox of a machine must be loaded identically
/// in a mailbox of another machine, whose nodes have other identifiers.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn general_part_sheet_roundtrip() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server_a = OpcUaTestServer::start(false, temp_dir.path(), machine_op10()).await;
    let server_b = OpcUaTestServer::start(true, temp_dir.path(), machine_op20()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config,
        db_creds,
        &[&server_a, &server_b],
    )
    .await;

    // Save the part sheets from the mailbox of the first machine.
    let mailbox_a = server_a.mailbox("TraceabilityMailboxOP10");
    let general_part_sheet = filled_general_part_sheet();
    for (name, value) in general_part_sheet.clone() {
        mailbox_a.write_general(name, value);
    }
    let response = mailbox_a
        .traceability_request(request::SAVE_PART_SHEETS)
        .await;
    assert_eq!(response, RESPONSE_OK);

    // Load the general part sheet in the second mailbox of the second machine.
    let mailbox_b_in = server_b.mailbox("TraceabilityMailboxOP20In");
    let mailbox_b_out = server_b.mailbox("TraceabilityMailboxOP20Out");
    mailbox_b_out.write_general(general::PART_ID, PART_ID);
    let response = mailbox_b_out
        .traceability_request(request::LOAD_GENERAL_PART_SHEET)
        .await;
    assert_eq!(response, RESPONSE_OK);

    assert_general_part_sheet_loaded(&mailbox_b_out, &general_part_sheet);

    // The other mailbox must not have been touched.
    for (name, value) in utils::opcua_server::general_part_sheet_defaults() {
        assert_eq!(
            mailbox_b_in.read_general(name),
            value,
            "variable {name} value"
        );
    }

    gateway.stop().await;
    server_a.stop().await;
    server_b.stop().await;
}

/// A general part sheet saved from a mailbox of a machine must be loaded identically
/// in another mailbox of the same machine.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn general_part_sheet_between_mailboxes() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op20()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config.clone(),
        db_creds,
        &[&server],
    )
    .await;

    let mailbox_in = server.mailbox("TraceabilityMailboxOP20In");
    let mailbox_out = server.mailbox("TraceabilityMailboxOP20Out");

    let general_part_sheet = filled_general_part_sheet();
    for (name, value) in general_part_sheet.clone() {
        mailbox_in.write_general(name, value);
    }
    let response = mailbox_in
        .traceability_request(request::SAVE_PART_SHEETS)
        .await;
    assert_eq!(response, RESPONSE_OK);

    mailbox_out.write_general(general::PART_ID, PART_ID);
    let response = mailbox_out
        .traceability_request(request::LOAD_GENERAL_PART_SHEET)
        .await;
    assert_eq!(response, RESPONSE_OK);

    assert_general_part_sheet_loaded(&mailbox_out, &general_part_sheet);

    // The archived general part sheet must record the mailbox it was saved from.
    let general_rows =
        wait_archived_part_sheets(&db_config, &db_config.general_part_sheet_table, PART_ID).await;
    assert_eq!(
        general_rows.len(),
        1,
        "general part sheets: {general_rows:?}"
    );
    assert_eq!(
        general_rows[0]["mailbox"],
        json!("TraceabilityMailboxOP20In")
    );

    gateway.stop().await;
    server.stop().await;
}

/// The part sheets archived on a save request must be the ones of the mailbox where
/// the request was made, the operation part sheet being found by its type.
#[tokio::test]
#[ignore = "requires the database and to be ran by cargo-nextest"]
async fn part_sheets_archived_from_their_mailbox() {
    let temp_dir = init();
    let (db_config, db_creds) = init_database();

    let server = OpcUaTestServer::start(false, temp_dir.path(), machine_op30()).await;

    let time_provider = TimeProvider::new(SimpleClock::new_frozen(), TimeZone::UTC);

    let gateway = TestGateway::start(
        temp_dir.path(),
        time_provider,
        db_config.clone(),
        db_creds,
        &[&server],
    )
    .await;

    // Give each mailbox its own part sheets values.
    for (mailbox, raw_part_ref, conforming, operator) in [
        ("TraceabilityMailboxOP30In", "1-498GR13", false, "Bob"),
        ("TraceabilityMailboxOP30OkOut", "1-498GR14", true, "Alice"),
        (
            "TraceabilityMailboxOP30ScrapOut",
            "1-498GR15",
            false,
            "Charlie",
        ),
    ] {
        let mailbox = server.mailbox(mailbox);
        mailbox.write_general(general::PART_ID, PART_ID);
        mailbox.write_general(general::RAW_PART_REF, raw_part_ref);
        mailbox.write_operation("Conforming", conforming);
        mailbox.write_operation("Operator", operator);
    }

    let response = server
        .mailbox("TraceabilityMailboxOP30OkOut")
        .traceability_request(request::SAVE_PART_SHEETS)
        .await;
    assert_eq!(response, RESPONSE_OK);

    let general_rows =
        wait_archived_part_sheets(&db_config, &db_config.general_part_sheet_table, PART_ID).await;
    assert_eq!(
        general_rows.len(),
        1,
        "general part sheets: {general_rows:?}"
    );
    assert_eq!(general_rows[0]["machine_id"], json!(server.name()));
    assert_eq!(
        general_rows[0]["mailbox"],
        json!("TraceabilityMailboxOP30OkOut")
    );
    assert_eq!(
        general_rows[0]["data"][general::RAW_PART_REF],
        json!("1-498GR14")
    );

    let operation_rows =
        wait_archived_part_sheets(&db_config, &db_config.operation_part_sheet_table, PART_ID).await;
    assert_eq!(
        operation_rows.len(),
        1,
        "operation part sheets: {operation_rows:?}"
    );
    assert_eq!(operation_rows[0]["machine_id"], json!(server.name()));
    assert_eq!(
        operation_rows[0]["mailbox"],
        json!("TraceabilityMailboxOP30OkOut")
    );
    assert_eq!(
        operation_rows[0]["data"],
        json!({"Conforming": true, "Operator": "Alice"})
    );

    gateway.stop().await;
    server.stop().await;
}
