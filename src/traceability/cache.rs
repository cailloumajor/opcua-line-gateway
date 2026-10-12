use std::num::TryFromIntError;
use std::sync::Arc;
use std::{fmt, io};

use jiff::Timestamp;
use jiff::civil::Date;
use opcua::types::{Context, NodeId, Variant};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle,
};
use strum::VariantArray;
use thiserror::Error;
use tracing::instrument;

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
    #[error("error serializing operation part sheet for database: {0}")]
    OperationSerialization(serde_json::Error),
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
    #[error("too much enqueued part sheets: {0}, max {1}")]
    TooMuchEnqueued(u64, u64),
}

/// Wrapper around a redb [`Database`], providing helper methods.
pub(crate) struct TraceabilityCache {
    /// Inner redb database.
    redb: Database,
    /// Threshold for draining not working state.
    queues_threshold: u64,
}

impl TraceabilityCache {
    /// Create a new [`TraceabilityCache`], provided a shareable [`Database`].
    pub(crate) fn new(redb: Database, queues_threshold: u64) -> Self {
        Self {
            redb,
            queues_threshold,
        }
    }

    /// Get the next serial number for the provided date.
    ///
    /// This function can block upon access to wrapped database.
    #[instrument(err, skip(self))]
    pub(super) fn next_serial(&self, date: Date) -> Result<u32, redb::Error> {
        let date_str = date.strftime("%Y%m%d").to_string();

        let write_txn = self.redb.begin_write()?;
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
    ) -> Result<Option<Vec<(NodeId, Variant)>>, GetGeneralPartSheetError> {
        let read_txn = self.redb.begin_read()?;
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
        operation: &[SavedPartSheetItem],
        ctx: &Context,
    ) -> Result<(), SavePartSheetsError> {
        let saved_at = Timestamp::now();

        // Encode general part sheet for caching and archiving.
        let cached_general = encode_part_sheet_for_cache(general, ctx)
            .map_err(SavePartSheetsError::ElementsCount)?;
        let json_general = encode_part_sheet_for_db(saved_at, &machine_id, part_id, general)
            .map_err(SavePartSheetsError::GeneralSerialization)?;

        // Encode operation part sheet for archiving.
        let json_operation = encode_part_sheet_for_db(saved_at, &machine_id, part_id, operation)
            .map_err(SavePartSheetsError::OperationSerialization)?;

        // Write encoded part sheets to the cache and/or queues.
        let write_txn = self.redb.begin_write()?;
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
            write_txn
                .open_table(OPERATION_PART_SHEET_QUEUE)?
                .insert(seq + 1, json_operation.as_str())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Returns success if enqueuing part sheet is possible, according to queues length
    /// threshold.
    #[instrument(err, skip_all)]
    pub(super) fn check_enqueuing_allowed(&self) -> Result<(), CheckEnqueuingError> {
        let mut total_enqueued = 0u64;

        let read_txn = self.redb.begin_read()?;

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

        if total_enqueued >= self.queues_threshold {
            return Err(CheckEnqueuingError::TooMuchEnqueued(
                total_enqueued,
                self.queues_threshold,
            ));
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

        let read_txn = self.redb.begin_read()?;
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
        let write_txn = self.redb.begin_write()?;
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

#[cfg(test)]
mod tests {
    use opcua::types::ContextOwned;
    use redb::backends::InMemoryBackend;

    use super::*;

    /// Create a [`TraceabilityCache`] backed by an in-memory database, provided
    /// the queues length threshold.
    fn in_memory_cache(queues_threshold: u64) -> TraceabilityCache {
        let database = Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .expect("creating an in-memory database should not fail");
        TraceabilityCache::new(database, queues_threshold)
    }

    /// Save the provided general and operation part sheets with a fixed machine
    /// identifier and the provided part identifier.
    fn save(
        cache: &TraceabilityCache,
        part_id: &str,
        general: &[SavedPartSheetItem],
        operation: &[SavedPartSheetItem],
    ) {
        let ctx = ContextOwned::default();
        cache
            .save_part_sheets(
                "MAC1".into(),
                part_id
                    .parse()
                    .expect("parsing part identifier should not fail"),
                general,
                operation,
                &ctx.context(),
            )
            .expect("saving part sheets should not fail");
    }

    #[test]
    fn next_serial() {
        let cache = in_memory_cache(0);

        const DAY_ONE: Date = Date::constant(1984, 12, 9);
        const DAY_TWO: Date = Date::constant(1985, 4, 23);

        let serial = cache
            .next_serial(DAY_ONE)
            .expect("getting a serial should not fail");
        assert_eq!(serial, 1);
        let serial = cache
            .next_serial(DAY_ONE)
            .expect("getting a serial should not fail");
        assert_eq!(serial, 2);
        let serial = cache
            .next_serial(DAY_TWO)
            .expect("getting a serial should not fail");
        assert_eq!(serial, 1);
    }

    #[test]
    fn check_enqueuing_allowed() {
        let cache = in_memory_cache(3);

        let part_sheet_item = (NodeId::new(0, 42), "SomeNode".into(), "somevalue".into());

        let ctx = ContextOwned::default();

        cache
            .save_part_sheets(
                "ID".into(),
                "00000000000000000000000".parse().unwrap(),
                std::slice::from_ref(&part_sheet_item),
                std::slice::from_ref(&part_sheet_item),
                &ctx.context(),
            )
            .expect("saving part sheets should not fail");

        cache
            .check_enqueuing_allowed()
            .expect("checking that enqueuing is allowed should not fail");

        cache
            .save_part_sheets(
                "ID".into(),
                "00000000000000000000000".parse().unwrap(),
                std::slice::from_ref(&part_sheet_item),
                std::slice::from_ref(&part_sheet_item),
                &ctx.context(),
            )
            .expect("saving part sheets should not fail");

        cache
            .check_enqueuing_allowed()
            .expect_err("checking that enqueuing is allowed should fail");
    }

    #[test]
    fn check_enqueuing_allowed_without_queue_tables() {
        let cache = in_memory_cache(1);

        cache
            .check_enqueuing_allowed()
            .expect("checking that enqueuing is allowed on empty cache should not fail");
    }

    #[test]
    fn check_enqueuing_allowed_zero_threshold() {
        let cache = in_memory_cache(0);

        let err = cache
            .check_enqueuing_allowed()
            .expect_err("checking that enqueuing is allowed should fail");
        assert!(matches!(err, CheckEnqueuingError::TooMuchEnqueued(0, 0)));
    }

    #[test]
    fn check_enqueuing_allowed_after_draining() {
        let cache = in_memory_cache(2);
        let part_sheet_item = (NodeId::new(0, 42), "SomeNode".into(), "somevalue".into());
        let items = std::slice::from_ref(&part_sheet_item);

        save(&cache, "00000000000000000000000", items, items);
        let err = cache
            .check_enqueuing_allowed()
            .expect_err("checking that enqueuing is allowed should fail");
        assert!(matches!(err, CheckEnqueuingError::TooMuchEnqueued(2, 2)));

        let (keys, _) = cache
            .get_queue_batch(QueueTable::General)
            .expect("getting a batch should not fail");
        cache
            .remove_entries(QueueTable::General, &keys)
            .expect("removing entries should not fail");

        cache
            .check_enqueuing_allowed()
            .expect("checking that enqueuing is allowed should not fail");
    }

    #[test]
    fn get_general_part_sheet_missing() {
        let cache = in_memory_cache(0);
        let ctx = ContextOwned::default();

        // Cache table does not exist yet.
        let err = cache
            .get_general_part_sheet("00000000000000000000000", &ctx.context())
            .expect_err("getting from a not existing table should fail");
        assert!(matches!(
            err,
            GetGeneralPartSheetError::RedbTable(redb::TableError::TableDoesNotExist(_))
        ));

        // Cache table exists, but not the entry.
        let item = (NodeId::new(1, 1), "first".into(), 1u32.into());
        save(
            &cache,
            "00000000000000000000000",
            std::slice::from_ref(&item),
            &[],
        );
        let part_sheet = cache
            .get_general_part_sheet("11111111111111111111111", &ctx.context())
            .expect("getting a general part sheet should not fail");
        assert_eq!(part_sheet, None);
    }

    #[test]
    fn save_and_get_general_part_sheet() {
        let cache = in_memory_cache(0);
        let ctx = ContextOwned::default();

        let general: &[SavedPartSheetItem] = &[
            (NodeId::new(1, 561), "first".into(), true.into()),
            (NodeId::new(2, "someval"), "second".into(), 42u16.into()),
        ];
        let operation: &[SavedPartSheetItem] =
            &[(NodeId::new(3, 43), "third".into(), "blabla".into())];
        save(&cache, "123456789XX422611100001", general, operation);

        let part_sheet = cache
            .get_general_part_sheet("123456789XX422611100001", &ctx.context())
            .expect("getting a general part sheet should not fail");
        let expected = vec![
            (NodeId::new(1, 561), true.into()),
            (NodeId::new(2, "someval"), 42u16.into()),
        ];
        assert_eq!(part_sheet, Some(expected));

        // Saving again with the same part identifier overwrites cached general part sheet.
        let general: &[SavedPartSheetItem] = &[(NodeId::new(1, 561), "first".into(), false.into())];
        save(&cache, "123456789XX422611100001", general, operation);

        let part_sheet = cache
            .get_general_part_sheet("123456789XX422611100001", &ctx.context())
            .expect("getting a general part sheet should not fail");
        assert_eq!(part_sheet, Some(vec![(NodeId::new(1, 561), false.into())]));
    }

    #[test]
    fn get_queue_batch_without_queue_tables() {
        let cache = in_memory_cache(0);

        for queue_table in QueueTable::VARIANTS {
            let (keys, body) = cache
                .get_queue_batch(*queue_table)
                .expect("getting a batch should not fail");
            assert!(keys.is_empty(), "{queue_table} keys should be empty");
            assert!(body.is_empty(), "{queue_table} body should be empty");
        }
    }

    #[test]
    fn save_part_sheets_enqueue_rows() {
        let cache = in_memory_cache(0);

        let general: &[SavedPartSheetItem] = &[(NodeId::new(1, 1), "first".into(), true.into())];
        let operation: &[SavedPartSheetItem] =
            &[(NodeId::new(1, 2), "second".into(), 42u16.into())];
        save(&cache, "00000000000000000000000", general, operation);
        save(&cache, "11111111111111111111111", general, operation);

        let (general_keys, general_body) = cache
            .get_queue_batch(QueueTable::General)
            .expect("getting a batch should not fail");
        let (operation_keys, operation_body) = cache
            .get_queue_batch(QueueTable::Operation)
            .expect("getting a batch should not fail");

        // Each save uses two consecutive sequence numbers, shared by both queues.
        assert_eq!(general_keys, [0, 2]);
        assert_eq!(operation_keys, [1, 3]);

        // Bodies are in `JSON Lines` format, in enqueuing order.
        let check_body = |body: &str, name: &str, value: serde_json::Value| {
            assert!(body.ends_with('\n'), "body should end with a newline");
            let rows: Vec<serde_json::Value> = body
                .lines()
                .map(|line| serde_json::from_str(line).expect("row should be valid JSON"))
                .collect();
            assert_eq!(rows.len(), 2);
            for (row, part_id) in rows
                .iter()
                .zip(["00000000000000000000000", "11111111111111111111111"])
            {
                assert_eq!(row["machine_id"], "MAC1");
                assert_eq!(row["part_id"], part_id);
                assert!(row["saved_at"].is_string());
                assert_eq!(row["data"], serde_json::json!({ name: value }));
            }
        };
        check_body(&general_body, "first", serde_json::json!(true));
        check_body(&operation_body, "second", serde_json::json!(42));
    }

    #[test]
    fn remove_entries() {
        let cache = in_memory_cache(0);
        let part_sheet_item = (NodeId::new(0, 42), "SomeNode".into(), "somevalue".into());
        let items = std::slice::from_ref(&part_sheet_item);

        for _ in 0..3 {
            save(&cache, "00000000000000000000000", items, items);
        }

        // Remove only part of the batch, including an unknown key.
        cache
            .remove_entries(QueueTable::General, &[0, 4, 99])
            .expect("removing entries should not fail");

        let (general_keys, general_body) = cache
            .get_queue_batch(QueueTable::General)
            .expect("getting a batch should not fail");
        assert_eq!(general_keys, [2]);
        assert_eq!(general_body.lines().count(), 1);

        // Other queue is left untouched.
        let (operation_keys, _) = cache
            .get_queue_batch(QueueTable::Operation)
            .expect("getting a batch should not fail");
        assert_eq!(operation_keys, [1, 3, 5]);
    }

    #[test]
    fn queue_sequence_not_reset_after_draining() {
        let cache = in_memory_cache(0);
        let part_sheet_item = (NodeId::new(0, 42), "SomeNode".into(), "somevalue".into());
        let items = std::slice::from_ref(&part_sheet_item);

        save(&cache, "00000000000000000000000", items, items);

        for queue_table in QueueTable::VARIANTS {
            let (keys, _) = cache
                .get_queue_batch(*queue_table)
                .expect("getting a batch should not fail");
            cache
                .remove_entries(*queue_table, &keys)
                .expect("removing entries should not fail");
        }

        save(&cache, "00000000000000000000000", items, items);

        let (general_keys, _) = cache
            .get_queue_batch(QueueTable::General)
            .expect("getting a batch should not fail");
        let (operation_keys, _) = cache
            .get_queue_batch(QueueTable::Operation)
            .expect("getting a batch should not fail");
        assert_eq!(general_keys, [2]);
        assert_eq!(operation_keys, [3]);
    }
}
