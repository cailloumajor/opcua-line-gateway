use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};

use anyhow::{Context as _, anyhow};
use clickhouse::Client;
use futures_util::StreamExt;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Builder as HyperClientBuilder;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use opcua_line_gateway_config::{TlsConfig, TraceabilityDatabaseConfig};
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use thiserror::Error;
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::wrappers::IntervalStream;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Level, debug, error, info, info_span, instrument};

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

/// CA and client certificate files contents memory.
#[derive(Debug, Default)]
struct LoadedTls {
    ca_cert: Vec<u8>,
    client_cert: Vec<u8>,
}

/// TLS state for database client.
struct TlsState {
    paths: Arc<TlsConfig>,
    loaded: Arc<LoadedTls>,
}

/// Database for traceability data archiving.
pub(crate) struct TraceabilityDatabase {
    /// Traceability database configuration.
    config: TraceabilityDatabaseConfig,
    /// ClickHouse client.
    client: Client,
    /// TLS state of the database client, if applicable.
    tls_state: Option<TlsState>,
    /// A shareable handle to the traceability cache.
    cache: Arc<TraceabilityCache>,
}

impl TraceabilityDatabase {
    /// Create a new [`TraceabilityDatabase`], provided ClickHouse client configuration,
    /// credentials and traceability cache.
    ///
    /// # Errors
    ///
    /// An error is returned if reading the password from the configured file fails.
    pub(crate) fn from_config(
        config: &TraceabilityDatabaseConfig,
        password: Option<SecretString>,
        cache: Arc<TraceabilityCache>,
    ) -> anyhow::Result<Self> {
        let (client, tls_state) = match (&config.tls, password) {
            (None, Some(password)) => (build_client_no_tls(config, password), None),
            (Some(tls), None) => {
                let (loaded, tls_config) = reload_tls_config(tls, &Default::default())
                    .context("Failed to load TLS configuration")?
                    .expect("loading TLS configuration from empty state should not be None");
                let state = TlsState {
                    paths: Arc::new(tls.clone()),
                    loaded: Arc::new(loaded),
                };
                (build_client_tls(config, tls_config), Some(state))
            }
            _ => {
                return Err(anyhow!(
                    "Invalid provided configuration, should have TLS or password"
                ));
            }
        };

        Ok(Self {
            config: config.clone(),
            client,
            tls_state,
            cache,
        })
    }

    /// Start a task to periodically drain the part sheets to the database, according
    /// to provided period. Runs forever until provided shutdown is triggered.
    ///
    /// Returns the receiver side of a channel where the result (successful or not)
    /// of the first run will be sent.
    pub(crate) async fn drain_part_sheets_task(
        mut self,
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

                    if let Some(tls) = &mut self.tls_state {
                        // Error path is not handled here, it will be logged by the
                        // function `instrument` attribute.
                        if let Ok(Some((loaded, tls_config))) = reload_tls_config_async(tls).await {
                            self.client = build_client_tls(&self.config, tls_config);
                            tls.loaded = Arc::new(loaded);
                        }
                    }

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
            QueueTable::General => &self.config.general_part_sheet_table,
            QueueTable::Operation => &self.config.operation_part_sheet_table,
        }
    }
}

/// Errors that can occur when building TLS client config.
#[derive(Debug, Error)]
enum TlsConfigError {
    #[error("invalid CA certificate: {0}")]
    Ca(rustls::pki_types::pem::Error),
    #[error("CA certificate not accepted as trust anchor: {0}")]
    TrustAnchor(rustls::Error),
    #[error("no CA certificate found")]
    NoCa,
    #[error("invalid client certificate: {0}")]
    ClientCert(rustls::pki_types::pem::Error),
    #[error("invalid client private key: {0}")]
    ClientKey(rustls::pki_types::pem::Error),
    #[error("client certificate and private key rejected: {0}")]
    ClientAuth(rustls::Error),
    #[error("error reading CA certificate file: {0}")]
    ReadCaCert(io::Error),
    #[error("error reading client certificate file: {0}")]
    ReadClientCert(io::Error),
    #[error("error reading client private key file: {0}")]
    ReadClientKey(io::Error),
    #[error("error joining TLS reload task: {0}")]
    ReloadTask(JoinError),
}

/// Build the TLS [`ClientConfig`], provided CA, cert, and key.
fn build_tls_config(
    ca_pem: &[u8],
    cert_pem: &[u8],
    key_pem: &SecretSlice<u8>,
) -> Result<ClientConfig, TlsConfigError> {
    let mut roots = RootCertStore::empty();
    for cert_result in CertificateDer::pem_slice_iter(ca_pem) {
        let cert = cert_result.map_err(TlsConfigError::Ca)?;
        roots.add(cert).map_err(TlsConfigError::TrustAnchor)?;
    }
    if roots.is_empty() {
        return Err(TlsConfigError::NoCa);
    }

    let chain = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(TlsConfigError::ClientCert)?;

    let key = PrivateKeyDer::from_pem_slice(key_pem.expose_secret())
        .map_err(TlsConfigError::ClientKey)?;

    let provider = Arc::new(aws_lc_rs::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("default crypto provider should support default protocol versions")
        .with_root_certificates(roots)
        .with_client_auth_cert(chain, key)
        .map_err(TlsConfigError::ClientAuth)
}

/// Reload TLS configuration, provided the configured paths and last loaded files
/// contents.
///
/// If TLS configuration was reloaded, returns the new certificates files contents
/// and the new TLS configuration, else return [`None`].
fn reload_tls_config(
    paths: &TlsConfig,
    loaded: &LoadedTls,
) -> Result<Option<(LoadedTls, ClientConfig)>, TlsConfigError> {
    let ca_cert = fs::read(&paths.ca_file).map_err(TlsConfigError::ReadCaCert)?;
    let client_cert = fs::read(&paths.cert_file).map_err(TlsConfigError::ReadClientCert)?;

    if ca_cert == loaded.ca_cert && client_cert == loaded.client_cert {
        // Nothing changed, TLS configuration is not reloaded.
        return Ok(None);
    }

    let key = SecretSlice::from(fs::read(&paths.key_file).map_err(TlsConfigError::ReadClientKey)?);

    let tls_config = build_tls_config(&ca_cert, &client_cert, &key)?;

    Ok(Some((
        LoadedTls {
            ca_cert,
            client_cert,
        },
        tls_config,
    )))
}

/// Asynchronous wrapper for reloading TLS configuration, to be used in the runtime
/// loop.
#[instrument(err(level = Level::WARN), skip_all)]
async fn reload_tls_config_async(
    state: &TlsState,
) -> Result<Option<(LoadedTls, ClientConfig)>, TlsConfigError> {
    let sent_paths = Arc::clone(&state.paths);
    let sent_loaded = Arc::clone(&state.loaded);

    let task = tokio::task::spawn_blocking(move || reload_tls_config(&sent_paths, &sent_loaded));

    task.await.map_err(TlsConfigError::ReloadTask)?
}

/// Create a builder for the HTTP client underlying the database client, closing
/// idle connections in the background.
fn hyper_client_builder() -> HyperClientBuilder {
    let mut builder = HyperClientBuilder::new(TokioExecutor::new());
    builder
        .pool_idle_timeout(Duration::from_secs(2))
        .pool_timer(TokioTimer::new());
    builder
}

/// Apply the settings common to all security modes to the provided database client.
fn traceability_database_client(client: Client, config: &TraceabilityDatabaseConfig) -> Client {
    client
        .with_url(config.url.as_str())
        .with_database(&config.default_database)
        .with_header("X-ClickHouse-User", &config.user)
        .with_setting("async_insert", "0")
}

/// Build the database client without TLS, provided the configuration and password.
fn build_client_no_tls(config: &TraceabilityDatabaseConfig, password: SecretString) -> Client {
    let client = Client::with_http_client(hyper_client_builder().build_http())
        .with_header("X-ClickHouse-Key", password.expose_secret());
    traceability_database_client(client, config)
}

/// Build the database client with TLS, provided the configuration and TLS config.
fn build_client_tls(config: &TraceabilityDatabaseConfig, tls_config: ClientConfig) -> Client {
    let connector = HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_only()
        .enable_http1()
        .build();
    let client = Client::with_http_client(hyper_client_builder().build(connector))
        .with_header("X-ClickHouse-SSL-Certificate-Auth", "on");
    traceability_database_client(client, config)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use indoc::indoc;

    use super::*;

    // Throwaway test PKI: a CA and two client certificates signed by it.
    const CA_PEM: &[u8] = include_bytes!("testdata/ca.pem");
    const CERT_PEM: &[u8] = include_bytes!("testdata/cert.pem");
    const KEY_PEM: &[u8] = include_bytes!("testdata/key.pem");
    const OTHER_CERT_PEM: &[u8] = include_bytes!("testdata/other-cert.pem");
    const OTHER_KEY_PEM: &[u8] = include_bytes!("testdata/other-key.pem");

    mod build_tls_config {
        use super::*;

        const TRUNCATED_CERT_PEM: &[u8] = b"-----BEGIN CERTIFICATE-----\nMIIB\n";

        #[test]
        fn invalid_ca() {
            let res = build_tls_config(TRUNCATED_CERT_PEM, CERT_PEM, &KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::Ca(_)));
        }

        #[test]
        fn rejected_trust_anchor() {
            const INVALID_DER_CA_PEM: &[u8] = indoc! {b"
                -----BEGIN CERTIFICATE-----\naGVsbG8=\n-----END CERTIFICATE-----\n
            "};

            let res = build_tls_config(INVALID_DER_CA_PEM, CERT_PEM, &KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::TrustAnchor(_)));
        }

        #[test]
        fn no_ca() {
            let res = build_tls_config(&[], CERT_PEM, &KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::NoCa));
        }

        #[test]
        fn bad_client_cert() {
            let res = build_tls_config(CA_PEM, TRUNCATED_CERT_PEM, &KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::ClientCert(_)));
        }

        #[test]
        fn bad_client_key() {
            const TRUNCATED_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\nMIGHAgEA\n";

            let res = build_tls_config(CA_PEM, CERT_PEM, &TRUNCATED_KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::ClientKey(_)));
        }

        #[test]
        fn cert_and_key_mismatch() {
            let res = build_tls_config(CA_PEM, OTHER_CERT_PEM, &KEY_PEM.to_vec().into());

            assert_matches!(res, Err(TlsConfigError::ClientAuth(_)));
        }

        #[test]
        fn success() {
            build_tls_config(CA_PEM, CERT_PEM, &KEY_PEM.to_vec().into())
                .expect("building TLS config with valid data should not fail");
        }
    }

    mod reload_tls_config {
        use std::path::Path;

        use super::*;

        fn write_tls_files(dir: &Path, ca: &[u8], cert: &[u8], key: &[u8]) -> TlsConfig {
            let paths = TlsConfig {
                ca_file: dir.join("ca.pem"),
                cert_file: dir.join("cert.pem"),
                key_file: dir.join("key.pem"),
            };
            fs::write(&paths.ca_file, ca).unwrap();
            fs::write(&paths.cert_file, cert).unwrap();
            fs::write(&paths.key_file, key).unwrap();
            paths
        }

        #[test]
        fn first_load() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);

            let (loaded, _) = reload_tls_config(&tls_config, &Default::default())
                .expect("first TLS config reloading should not fail")
                .expect("first reloading should produce a config");

            assert_eq!(loaded.ca_cert, CA_PEM);
            assert_eq!(loaded.client_cert, CERT_PEM);
        }

        #[test]
        fn same_contents() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let reloaded = reload_tls_config(&tls_config, &loaded)
                .expect("reloading already loaded TLS should not fail");

            assert!(reloaded.is_none());
        }

        #[test]
        fn key_file_not_read() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let mut tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);
            tls_config.key_file = temp_dir.path().join("nonexistent.pem");
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let reloaded = reload_tls_config(&tls_config, &loaded)
                .expect("reloading already loaded TLS should not fail");

            assert!(reloaded.is_none());
        }

        #[test]
        fn ca_changed() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);
            let loaded = LoadedTls {
                ca_cert: Vec::new(),
                client_cert: CERT_PEM.to_vec(),
            };

            let (loaded, _) = reload_tls_config(&tls_config, &loaded)
                .expect("loading new CA should not fail")
                .expect("loading new CA should produce a config");

            assert_eq!(loaded.ca_cert, CA_PEM);
        }

        #[test]
        fn certificate_and_key_changed() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let tls_config =
                write_tls_files(temp_dir.path(), CA_PEM, OTHER_CERT_PEM, OTHER_KEY_PEM);
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let (loaded, _) = reload_tls_config(&tls_config, &loaded)
                .expect("loading new certificate and key should not fail")
                .expect("loading new cert and key should produce a config");

            assert_eq!(loaded.client_cert, OTHER_CERT_PEM);
        }

        #[test]
        fn missing_ca_file() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let mut tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);
            tls_config.ca_file = temp_dir.path().join("nonexistent.pem");
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let res = reload_tls_config(&tls_config, &loaded);

            assert_matches!(res, Err(TlsConfigError::ReadCaCert(_)));
        }

        #[test]
        fn missing_cert_file() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let mut tls_config = write_tls_files(temp_dir.path(), CA_PEM, CERT_PEM, KEY_PEM);
            tls_config.cert_file = temp_dir.path().join("nonexistent.pem");
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let res = reload_tls_config(&tls_config, &loaded);

            assert_matches!(res, Err(TlsConfigError::ReadClientCert(_)));
        }

        #[test]
        fn missing_key_file() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let mut tls_config =
                write_tls_files(temp_dir.path(), CA_PEM, OTHER_CERT_PEM, OTHER_KEY_PEM);
            tls_config.key_file = temp_dir.path().join("nonexistent.pem");
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            let res = reload_tls_config(&tls_config, &loaded);

            assert_matches!(res, Err(TlsConfigError::ReadClientKey(_)));
        }

        #[test]
        fn build_error() {
            let temp_dir = tempfile::tempdir().expect("creating temp directory should not fail");
            let tls_config = write_tls_files(temp_dir.path(), CA_PEM, OTHER_CERT_PEM, KEY_PEM);
            let loaded = LoadedTls {
                ca_cert: CA_PEM.to_vec(),
                client_cert: CERT_PEM.to_vec(),
            };

            reload_tls_config(&tls_config, &loaded)
                .expect_err("reloading with a build error should fail");
        }
    }
}
