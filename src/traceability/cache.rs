use std::num::TryFromIntError;
use std::sync::Arc;
use std::{fmt, io};

use jiff::Timestamp;
use jiff::civil::Date;
use opcua::types::{Context, Variant};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle,
};
use strum::VariantArray;
use thiserror::Error;
use tracing::{instrument, warn};

use crate::traceability::part_sheet::encode_part_sheet_for_db;

use super::part_sheet::{
    SavedPartSheetItem, decode_cached_part_sheet, encode_part_sheet_for_cache,
};

/// The lower limit on enqueued part sheets from which enqueueing will not be allowed.
const QUEUES_THRESHOLD: u64 = 20;

/// Table definition for the daily serial numbers.
const SERIAL_TABLE: TableDefinition<&str, u32> = TableDefinition::new("daily_serial");

/// Cached general part sheets.
const GENERAL_PART_SHEET_CACHE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("general_part_sheet");

/// Global monotonic sequence number for part sheets archiving queue. Must NEVER be reset,
/// even if the queue is empty.
const QUEUE_SEQ: TableDefinition<(), u64> = TableDefinition::new("archive_seq");
/// General part sheet `JSONEachRow` rows waiting to be inserted in database.
const GENERAL_PART_SHEET_QUEUE: TableDefinition<u64, &str> = TableDefinition::new("general_queue");
/// Operation part sheet `JSONEachRow` rows waiting to be inserted in database.
const OPERATION_PART_SHEET_QUEUE: TableDefinition<u64, &str> =
    TableDefinition::new("operation_queue");

/// Part sheet queue table dispatch. Allows to reduce exposition of symbols from
/// this module (e.g. table definitions).
#[derive(Clone, Copy, VariantArray)]
pub(super) enum QueueTable {
    /// General part sheet queue table.
    General,
    /// Operation part sheet queue table.
    Operation,
}

impl QueueTable {
    fn table_definition<'a>(self) -> TableDefinition<'static, u64, &'a str> {
        match self {
            Self::General => GENERAL_PART_SHEET_QUEUE,
            Self::Operation => OPERATION_PART_SHEET_QUEUE,
        }
    }
}

impl fmt::Display for QueueTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.table_definition().name())
    }
}

/// Errors that can occur during retrieval of general part sheet from the cache.
#[derive(Debug, Error)]
pub(super) enum GetGeneralPartSheetError {
    #[error(transparent)]
    RedbTransaction(#[from] redb::TransactionError),
    #[error(transparent)]
    RedbTable(#[from] redb::TableError),
    #[error(transparent)]
    RedbStorage(#[from] redb::StorageError),
    #[error("error decoding general part sheet")]
    Decoding(#[source] io::Error),
}

/// Errors that can occur during insertion of general part sheet in the cache.
#[derive(Debug, Error)]
pub(super) enum SavePartSheetsError {
    #[error("number of elements does not fit in an u16: {0}")]
    ElementsCount(TryFromIntError),
    #[error("error serializing general part sheet for database: {0}")]
    GeneralSerialization(serde_json::Error),
    #[error(transparent)]
    RedbTransaction(#[from] redb::TransactionError),
    #[error(transparent)]
    RedbTable(#[from] redb::TableError),
    #[error(transparent)]
    RedbStorage(#[from] redb::StorageError),
    #[error(transparent)]
    RedbCommit(#[from] redb::CommitError),
}

/// Errors that can occur during checking if enqueuing is allowed.
#[derive(Debug, Error)]
pub(super) enum CheckEnqueuingError {
    #[error(transparent)]
    RedbTransaction(#[from] redb::TransactionError),
    #[error(transparent)]
    RedbTable(#[from] redb::TableError),
    #[error(transparent)]
    RedbStorage(#[from] redb::StorageError),
    #[error("too much enqueued part sheets: {0}, max {QUEUES_THRESHOLD}")]
    TooMuchEnqueued(u64),
}

/// Wrapper around a redb [`Database`], providing helper methods.
pub(crate) struct TraceabilityCache(Database);

impl TraceabilityCache {
    /// Create a new [`TraceabilityCache`], provided a shareable [`Database`].
    pub(crate) fn new(db: Database) -> Self {
        Self(db)
    }

    /// Get the next serial number for the provided date.
    ///
    /// This function can block upon access to wrapped database.
    #[instrument(err, skip(self))]
    pub(super) fn next_serial(&self, today: Date) -> Result<u32, redb::Error> {
        let date_str = today.strftime("%Y%m%d").to_string();

        let write_txn = self.0.begin_write()?;
        let next = {
            let mut table = write_txn.open_table(SERIAL_TABLE)?;
            let next = table
                .get(date_str.as_str())?
                .map(|v| v.value() + 1)
                .unwrap_or(1);
            table.insert(date_str.as_str(), next)?;
            next
        };
        write_txn.commit()?;

        Ok(next)
    }

    /// Get a general part sheet from the cache, provided the part identifier and
    /// an OPC-UA encoding context.
    #[instrument(err, skip_all, fields(part_id = part_id))]
    pub(super) fn get_general_part_sheet(
        &self,
        part_id: &str,
        ctx: &Context,
    ) -> Result<Option<Vec<(u32, Variant)>>, GetGeneralPartSheetError> {
        let read_txn = self.0.begin_read()?;
        let table = read_txn.open_table(GENERAL_PART_SHEET_CACHE)?;
        let value_guard = table.get(part_id)?;

        value_guard
            .map(|g| {
                decode_cached_part_sheet(g.value(), ctx).map_err(GetGeneralPartSheetError::Decoding)
            })
            .transpose()
    }

    /// Provided general and operation part_sheets, save the general one to the cache
    /// and enqueue both for database insertion.
    ///
    /// This function blocks upon access to wrapped database.
    #[instrument(err, skip_all, fields(part_id = %part_id))]
    pub(super) fn save_part_sheets(
        &self,
        machine_id: Arc<str>,
        part_id: AsciiDigitsOrUpper<23>,
        general: &[SavedPartSheetItem],
        ctx: &Context,
    ) -> Result<(), SavePartSheetsError> {
        let saved_at = Timestamp::now();

        let cached_general = encode_part_sheet_for_cache(general, ctx)
            .map_err(SavePartSheetsError::ElementsCount)?;
        let json_general = encode_part_sheet_for_db(saved_at, &machine_id, part_id, general)
            .map_err(SavePartSheetsError::GeneralSerialization)?;

        // TODO: encode the operation part sheet (adding required parameters to this function)
        //       to JSON and enqueue them in to-be-created tables.
        warn!(msg = "operation part sheet insertion to database is not yet implemented");

        let write_txn = self.0.begin_write()?;
        {
            let mut seq_table = write_txn.open_table(QUEUE_SEQ)?;
            let seq = seq_table
                .entry(())?
                // Increment by two, because we use two sequence numbers below.
                .and_modify(|v| v.insert(v.value() + 2))?
                .or_insert(0)?
                .value();

            write_txn
                .open_table(GENERAL_PART_SHEET_CACHE)?
                .insert(part_id.as_str(), cached_general.as_slice())?;
            write_txn
                .open_table(GENERAL_PART_SHEET_QUEUE)?
                .insert(seq, json_general.as_str())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Returns success if enqueuing part sheet is possible, according to queues length
    /// threshold.
    #[instrument(err, skip_all)]
    pub(super) fn check_enqueuing_allowed(&self) -> Result<(), CheckEnqueuingError> {
        let mut total_enqueued = 0u64;

        let read_txn = self.0.begin_read()?;

        for queue_table in QueueTable::VARIANTS {
            let table_result = read_txn.open_untyped_table(queue_table.table_definition());
            if let Err(redb::TableError::TableDoesNotExist(_)) = table_result {
                // Count a not existing table as an empty one.
                continue;
            }
            let table = table_result?;
            let len = table.len()?;

            total_enqueued += len;
        }

        if total_enqueued >= QUEUES_THRESHOLD {
            return Err(CheckEnqueuingError::TooMuchEnqueued(total_enqueued));
        }

        Ok(())
    }

    /// Get a batch from a rows queue, as a couple of keys (sequence numbers)
    /// and the `JSON Lines` body, provided the queue table to get a batch from.
    pub(super) fn get_queue_batch(
        &self,
        queue_table: QueueTable,
    ) -> Result<(Vec<u64>, String), redb::Error> {
        let mut keys = Vec::new();
        let mut body = String::new();

        let read_txn = self.0.begin_read()?;
        let table_result = read_txn.open_table(queue_table.table_definition());
        if let Err(redb::TableError::TableDoesNotExist(_)) = table_result {
            return Ok(Default::default());
        }
        let table = table_result?;
        for item in table.iter()? {
            let (k, v) = item?;
            keys.push(k.value());
            body.push_str(v.value());
            body.push('\n');
        }

        Ok((keys, body))
    }

    /// Remove entries of a batch with provided keys from the queue with provided
    /// table definition.
    pub(super) fn remove_entries(
        &self,
        queue_table: QueueTable,
        keys: &[u64],
    ) -> Result<(), redb::Error> {
        let write_txn = self.0.begin_write()?;
        {
            let mut table = write_txn.open_table(queue_table.table_definition())?;
            for key in keys {
                table.remove(key)?;
            }
        }
        write_txn.commit()?;

        Ok(())
    }
}
