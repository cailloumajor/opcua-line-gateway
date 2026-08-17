use std::io::{self, Read};

use jiff::civil::Date;
use leb128::write::unsigned_len;
use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use thiserror::Error;
use tracing::instrument;

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

/// Cloneable wrapper around a shareable [`Database`], providing helper methods.
pub(crate) struct TraceabilityCache {
    db: Database,
}

impl TraceabilityCache {
    /// Create a new [`TraceabilityCache`], provided a shareable [`Database`].
    pub(crate) fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get the next serial number for the provided date.
    ///
    /// This function can block upon access to wrapped database.
    #[instrument(err, skip(self))]
    pub(super) fn next_serial(&self, today: Date) -> Result<u32, redb::Error> {
        let date_str = today.strftime("%Y%m%d").to_string();

        let write_txn = self.db.begin_write()?;
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
    ) -> Result<Option<Vec<(u32, Variant)>>, GetGeneralPartSheetError> {
        let read_txn = self.db.begin_read().map_err(redb::Error::from)?;
        let table = read_txn
            .open_table(GENERAL_PART_SHEET_TABLE)
            .map_err(redb::Error::from)?;
        let value_guard = table.get(part_id).map_err(redb::Error::from)?;

        value_guard
            .map(|g| decode_part_sheet(g.value(), ctx).map_err(GetGeneralPartSheetError::Decoding))
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
        part_sheet: &[(&u32, &Variant)],
        ctx: &Context,
    ) -> Result<(), redb::Error> {
        let encoded = encode_part_sheet(part_sheet, ctx);

        let write_txn = self.db.begin_write()?;
        write_txn
            .open_table(GENERAL_PART_SHEET_TABLE)?
            .insert(part_id, encoded.as_slice())?;
        write_txn.commit()?;

        Ok(())
    }
}

/// Encode a part sheet, provided as an iterator of node identifier and [`Variant`],
/// in the provided reusable buffer, using provided OPC-UA encoding context.
#[instrument(skip_all)]
fn encode_part_sheet(part_sheet: &[(&u32, &Variant)], ctx: &Context) -> Vec<u8> {
    let count: u64 = part_sheet
        .len()
        .try_into()
        .expect("provided elements length should fit in an u64");

    let encoded_elements_size = part_sheet
        .iter()
        .map(|(id, variant)| size_of_val(*id) + variant.byte_len(ctx))
        .sum::<usize>();

    let mut buf: Vec<u8> = Vec::with_capacity(unsigned_len(count) + encoded_elements_size);

    // Encode the number of elements (unsigned LEB128).
    leb128::write::unsigned(&mut buf, count).expect("writing to a Vec should not fail");

    for (id, variant) in part_sheet {
        // Encode the node identifier (32 bits little endian).
        buf.extend_from_slice(&id.to_le_bytes());
        // Encode the variant (OPC-UA binary encoding).
        variant
            .encode(&mut buf, ctx)
            .expect("writing to a Vec should not fail");
    }

    buf
}

/// Consume and decode this [`CachedPartSheet`], returning a vector of pairs of [`Variant`]
/// and node identifier.
#[instrument(err, skip_all)]
fn decode_part_sheet(mut buf: &[u8], ctx: &Context) -> io::Result<Vec<(u32, Variant)>> {
    // Decode the number of elements (unsigned LEB128).
    let count = leb128::read::unsigned(&mut buf).map_err(|e| match e {
        leb128::read::Error::IoError(err) => err,
        leb128::read::Error::Overflow => io::Error::other(e),
    })?;

    let cap = usize::try_from(count).expect("number of elements should fit in an usize");

    let mut out = Vec::with_capacity(cap);

    for _ in 0..count {
        // Decode the node identifier (32 bits little endian).
        let mut id_bytes = [0u8; 4];
        buf.read_exact(&mut id_bytes)?;
        let id = u32::from_le_bytes(id_bytes);
        // Decode the variant (OPC-UA binary encoding).
        let variant = Variant::decode(&mut buf, ctx)?;

        out.push((id, variant));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use opcua::types::ContextOwned;

    use super::*;

    #[test]
    fn part_sheet_encode_decode() {
        let part_sheet = &[
            (&561, &true.into()),
            (&98, &42u16.into()),
            (&43, &"blabla".into()),
        ];
        let ctx = ContextOwned::default();

        let encoded = encode_part_sheet(part_sheet, &ctx.context());
        let decoded =
            decode_part_sheet(&encoded, &ctx.context()).expect("decoding should not fail");

        let expected = &[
            (561, true.into()),
            (98, 42u16.into()),
            (43, "blabla".into()),
        ];

        assert_eq!(decoded, expected);
    }
}
