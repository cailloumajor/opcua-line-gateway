use std::env;
use std::time::Duration;

use duct::cmd;
use indoc::formatdoc;
use opcua_line_gateway_config::TraceabilityDatabaseConfig;
use rand::distr::{Alphanumeric, SampleString};
use secrecy::SecretString;
use serde_json::Value;

/// Initialize the database, i.e. creates a database (a.k.a. schema) according
/// to the test name.
///
/// Return the database configuration and password.
pub fn init_database() -> (TraceabilityDatabaseConfig, SecretString) {
    let test_name = env::var("NEXTEST_TEST_NAME")
        .expect("NEXTEST_TEST_NAME environment variable should be set");
    let database = format!("traceability_it_{test_name}");

    let http_port = env::var("CLICKHOUSE_TEST_HTTP_PORT")
        .expect("CLICKHOUSE_TEST_HTTP_PORT should be set by the Nextest task");
    let tcp_port = env::var("CLICKHOUSE_TEST_TCP_PORT")
        .expect("CLICKHOUSE_TEST_TCP_PORT should be set by the Nextest task");

    // Drop the testing database.
    let drop_cmd = cmd!(
        "clickhouse",
        "client",
        "--host",
        "127.0.0.1",
        "--port",
        &tcp_port,
        "--query",
        format!("DROP DATABASE IF EXISTS {database}")
    );
    drop_cmd
        .run()
        .expect("dropping the traceability database should not fail");

    // Create database and tables for traceability tests.
    let sql_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/clickhouse/tables/traceability.sql"
    );
    let tables_create_cmd = cmd!(
        "clickhouse",
        "client",
        "--host",
        "127.0.0.1",
        "--port",
        &tcp_port,
        format!("--param_database={database}"),
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
        "clickhouse",
        "client",
        "--host",
        "127.0.0.1",
        "--port",
        &tcp_port,
        format!("--param_password={password}"),
        "--query",
        query
    );
    user_create_cmd
        .run()
        .expect("executing user creation query should not fail");

    let config = TraceabilityDatabaseConfig {
        url: format!("http://127.0.0.1:{http_port}").parse().unwrap(),
        user,
        default_database: database,
        part_sheets_drain_period: Duration::from_millis(100),
        general_part_sheet_table: "part_sheet_general".into(),
        operation_part_sheet_table: "part_sheet_operation".into(),
    };

    (config, password.into())
}

/// Wait for the part sheets of the given part to have been archived in the given
/// table, and return them as JSON objects (`machine_id`, `mailbox` and `data` columns).
pub async fn wait_archived_part_sheets(
    db_config: &TraceabilityDatabaseConfig,
    table: &str,
    part_id: &str,
) -> Vec<Value> {
    let database = &db_config.default_database;
    let query = formatdoc! {"
        SELECT machine_id, mailbox, data FROM {database}.{table}
        WHERE part_id = {{part_id: String}}
        ORDER BY saved_at
        FORMAT JSONEachRow
    "};

    let tcp_port = env::var("CLICKHOUSE_TEST_TCP_PORT")
        .expect("CLICKHOUSE_TEST_TCP_PORT should be set by the Nextest task");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let output = cmd!(
                "clickhouse",
                "client",
                "--host",
                "127.0.0.1",
                "--port",
                &tcp_port,
                format!("--param_part_id={part_id}"),
                "--query",
                &query
            )
            .read()
            .expect("querying the archived part sheets should not fail");
            let rows = output
                .lines()
                .map(|line| serde_json::from_str(line).expect("rows should be valid JSON"))
                .collect::<Vec<Value>>();
            if !rows.is_empty() {
                return rows;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("part sheets of part {part_id} should be archived in {table}"))
}
