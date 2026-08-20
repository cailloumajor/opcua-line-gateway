use std::io;
use std::num::TryFromIntError;

use jiff::civil::Date;
use opcua::types::Context;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use thiserror::Error;
use tracing::instrument;

use super::part_sheet::CachedPartSheet;

/// Table definition for the daily serial numbers.
const SERIAL_TABLE: TableDefinition<&str, u32> = TableDefinition::new("daily_serial");

const GENERAL_PART_SHEET_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("general_part_sheet");

/// Errors that can occur during retrieval of general part sheet from the cache.
#[derive(Debug, Error)]
pub(super) enum GetGeneralPartSheetError {
    #[error(transparent)]
    Redb(#[from] redb::Error),
    #[error("error decoding general part sheet")]
    Decoding(#[source] io::Error),
}

/// Errors that can occur during insertion of general part sheet in the cache.
#[derive(Debug, Error)]
pub(super) enum InsertGeneralPartSheetError {
    #[error("number of elements does not fit in an u16: {0}")]
    ElementsCount(TryFromIntError),
    #[error(transparent)]
    Redb(#[from] redb::Error),
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
    #[instrument(err, skip_all, fields(part_id))]
    pub(super) fn get_general_part_sheet(
        &self,
        part_id: &str,
        ctx: &Context,
    ) -> Result<Option<CachedPartSheet>, GetGeneralPartSheetError> {
        let read_txn = self.0.begin_read().map_err(redb::Error::from)?;
        let table = read_txn
            .open_table(GENERAL_PART_SHEET_TABLE)
            .map_err(redb::Error::from)?;
        let value_guard = table.get(part_id).map_err(redb::Error::from)?;

        value_guard
            .map(|g| {
                CachedPartSheet::from_cache_encoding(g.value(), ctx)
                    .map_err(GetGeneralPartSheetError::Decoding)
            })
            .transpose()
    }

    /// Write a general part sheet in the cache, provided the part identifier,
    /// the part sheet as a slice of node identifier and variant, and an OPC-UA
    /// encoding context.
    ///
    /// This function can block upon access to wrapped database.
    #[instrument(err, skip_all, fields(part_id))]
    pub(super) fn insert_general_part_sheet(
        &self,
        part_id: &str,
        part_sheet: CachedPartSheet,
        ctx: &Context,
    ) -> Result<(), InsertGeneralPartSheetError> {
        let encoded = part_sheet
            .encode_for_cache(ctx)
            .map_err(InsertGeneralPartSheetError::ElementsCount)?;

        let write_txn = self.0.begin_write().map_err(redb::Error::from)?;
        write_txn
            .open_table(GENERAL_PART_SHEET_TABLE)
            .map_err(redb::Error::from)?
            .insert(part_id, encoded.as_slice())
            .map_err(redb::Error::from)?;
        write_txn.commit().map_err(redb::Error::from)?;

        Ok(())
    }
}
