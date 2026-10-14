use std::env;
use std::path::Path;
use std::time::Duration;

use duct::cmd;
use indoc::formatdoc;
use opcua_line_gateway_config::{Credential, TraceabilityDatabaseConfig};
use rand::distr::{Alphanumeric, SampleString};
use serde::Deserialize;

/// Name of the CLickHouse server testing instance.
const CH_SERVER_NAME: &str = "integration-tests";

/// Partially represents the output of local servers list from `clickhousectl`.
#[derive(Deserialize)]
struct ChctlServerList {
    servers: Vec<Server>,
}

#[derive(Deserialize)]
struct Server {
    name: String,
    http_port: Option<u16>,
}

/// Initialize the database, i.e. creates a database (a.k.a. schema) according
/// to the test name.
///
/// Return the database configuration and database credentials.
pub fn init_database() -> (TraceabilityDatabaseConfig, Credential) {
    let test_name = env::var("NEXTEST_TEST_NAME")
        .expect("NEXTEST_TEST_NAME environment variable should be set");
    let database = format!("traceability_it_{test_name}");

    let raw_json = cmd!("clickhousectl", "local", "server", "list", "--json")
        .read()
        .expect("running clickhousectl local servers listing command should not fail");
    let list: ChctlServerList = serde_json::from_str(&raw_json)
        .expect("deserializing clickhousectl JSON output should not fail");
    let port = list
        .servers
        .into_iter()
        .find_map(|server| {
            if server.name.eq(CH_SERVER_NAME) {
                server.http_port
            } else {
                None
            }
        })
        .expect("there should be a local ClickHouse test server");

    // Drop the testing database.
    let drop_cmd = cmd!(
        "clickhousectl",
        "local",
        "client",
        "--name",
        CH_SERVER_NAME,
        "--query",
        format!("DROP DATABASE IF EXISTS {database}")
    );
    drop_cmd
        .run()
        .expect("dropping the traceability database should not fail");

    // Create database and tables for traceability tests.
    let sql_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("clickhouse/tables/traceability.sql");
    let tables_create_cmd = cmd!(
        "clickhousectl",
        "local",
        "client",
        "--name",
        CH_SERVER_NAME,
        "--param_database",
        &database,
        "--queries-file",
        sql_path
    );
    tables_create_cmd
        .run()
        .expect("executing traceability tables creation SQL should not fail");

    // Create an user with a password, and assign him to the traceability role.
    let user = format!("test_{test_name}");
    let password = Alphanumeric.sample_string(&mut rand::rng(), 16);
    let query = formatdoc! {"
        CREATE USER OR REPLACE {user} IDENTIFIED BY {{password: String}};
        GRANT traceability_gateway TO {user};
    "};
    let user_create_cmd = cmd!(
        "clickhousectl",
        "local",
        "client",
        "--name",
        CH_SERVER_NAME,
        "--param_user",
        &user,
        "--param_password",
        &password,
        "--query",
        query
    );
    user_create_cmd
        .run()
        .expect("executing user creation query should not fail");

    let config = TraceabilityDatabaseConfig {
        url: format!("http://127.0.0.1:{port}"),
        default_database: database,
        part_sheets_drain_period: Duration::from_millis(100),
        general_part_sheet_table: "part_sheet_general".into(),
        operation_part_sheet_table: "part_sheet_operation".into(),
    };

    let credential = Credential { user, password };

    (config, credential)
}
