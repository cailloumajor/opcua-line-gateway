use std::io;
use std::num::TryFromIntError;
use std::sync::Arc;

use jiff::Timestamp;
use jiff::civil::Date;
use opcua::types::{Context, Variant};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use thiserror::Error;
use tracing::{instrument, warn};

use crate::traceability::part_sheet::encode_part_sheet_for_db;

use super::part_sheet::{
    SavedPartSheetItem, decode_cached_part_sheet, encode_part_sheet_for_cache,
};

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
}
