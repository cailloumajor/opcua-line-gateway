use std::sync::Arc;

use jiff::civil::Date;
use redb::{Database, ReadableTable, TableDefinition};

/// Table definition for the daily serial numbers.
const SERIAL_TABLE: TableDefinition<&str, u32> = TableDefinition::new("daily_serial");

/// Cloneable wrapper around a shareable [`Database`], providing helper methods.
#[derive(Clone)]
pub(super) struct TraceabilityCache(Arc<Database>);

impl TraceabilityCache {
    /// Create a new [`TraceabilityCache`], provided a shareable [`Database`].
    pub(super) fn new(db: Arc<Database>) -> Self {
        Self(db)
    }

    /// Get the next serial number for the provided date.
    ///
    /// This function can block upon access to wrapped database.
    pub(super) fn next_serial(&self, today: &Date) -> Result<u32, redb::Error> {
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
}
