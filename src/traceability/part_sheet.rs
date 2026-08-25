use std::num::TryFromIntError;
use std::sync::Arc;

use jiff::Timestamp;
use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use opcua_line_gateway_config::AsciiDigitsOrUpper;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use tracing::instrument;

use crate::opcua::SerializeVariant;

/// Type of an item in a part sheet to be cached and/or archived.
pub(super) type SavedPartSheetItem = (u32, Arc<str>, Variant);

/// Encode a part sheet, provided as an iterator of element, to the format used
/// for caching, returning the encoded bytes.
///
/// # Note
///
/// The provided iterator needs to be driven two times, so it will be cloned once.
/// To prevent performance penalties, ensure the iterator is cheaply cloneable.
///
/// # Errors
///
/// Return an error if the number of elements is too big (must fit in an [`u16`]).
#[instrument(skip_all)]
pub(super) fn encode_part_sheet_for_cache(
    part_sheet: &[SavedPartSheetItem],
    ctx: &Context,
) -> Result<Vec<u8>, TryFromIntError> {
    let count: u16 = part_sheet.len().try_into()?;

    let encoded_elements_size = part_sheet
        .iter()
        .map(|(id, _, variant)| id.byte_len(ctx) + variant.byte_len(ctx))
        .sum::<usize>();

    let mut buf: Vec<u8> = Vec::with_capacity(count.byte_len(ctx) + encoded_elements_size);

    // Encode the number of elements.
    count
        .encode(&mut buf, ctx)
        .expect("writing to a Vec should not fail");

    for (id, _, variant) in part_sheet {
        // Encode the node identifier.
        id.encode(&mut buf, ctx)
            .expect("writing to a Vec should not fail");
        // Encode the variant (OPC-UA binary encoding).
        variant
            .encode(&mut buf, ctx)
            .expect("writing to a Vec should not fail");
    }

    Ok(buf)
}

/// Decode a part sheet, i.e. a collection of elements, from cache encoding format.
///
/// # Errors
///
/// Return an error if something goes wrong during decoding.
#[instrument(err, skip_all)]
pub(super) fn decode_cached_part_sheet(
    mut buf: &[u8],
    ctx: &Context,
) -> std::io::Result<Vec<(u32, Variant)>> {
    // Decode the number of elements.
    let count = u16::decode(&mut buf, ctx)?;

    let mut out = Vec::with_capacity(count.into());

    for _ in 0..count {
        // Decode the node identifier.
        let id = u32::decode(&mut buf, ctx)?;
        // Decode the variant (OPC-UA binary encoding).
        let variant = Variant::decode(&mut buf, ctx)?;

        out.push((id, variant));
    }

    Ok(out)
}

/// Encode a part sheet, provided as an iterator of element, to the format used
/// for database insertion (`JSONEachRow`), returning the encoded bytes.
///
/// # Errors
///
/// Return an error if something goes wrong with serialization.
#[instrument(err, skip_all)]
pub(super) fn encode_part_sheet_for_db(
    saved_at: Timestamp,
    machine_id: &str,
    part_id: AsciiDigitsOrUpper<23>,
    part_sheet: &[SavedPartSheetItem],
) -> serde_json::Result<String> {
    let row = PartSheetRow {
        saved_at,
        machine_id,
        part_id,
        data: part_sheet,
    };

    serde_json::to_string(&row)
}

/// Utility struct to allow serializing a part sheet database row.
#[derive(Serialize)]
struct PartSheetRow<'a> {
    #[serde(serialize_with = "serialize_timestamp")]
    saved_at: Timestamp,
    machine_id: &'a str,
    part_id: AsciiDigitsOrUpper<23>,
    #[serde(serialize_with = "serialize_part_sheet")]
    data: &'a [SavedPartSheetItem],
}

/// Serialize a timestamp in a ClickHouse idiomatic format to use with DateTime64 column.
fn serialize_timestamp<S>(ts: &Timestamp, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(&ts.strftime("%F %T%3.f"))
}

fn serialize_part_sheet<S>(
    part_sheet: &[SavedPartSheetItem],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(part_sheet.len()))?;
    for (_, name, value) in part_sheet {
        map.serialize_entry(name.as_ref(), &SerializeVariant(value))?;
    }
    map.end()
}

#[cfg(test)]
mod tests {
    use opcua::types::ContextOwned;
    use serde_test::{Token, assert_ser_tokens};

    use super::*;

    #[test]
    fn cache_encoding_roudtrip() {
        let part_sheet: &[SavedPartSheetItem] = &[
            (561, "".into(), true.into()),
            (98, "".into(), 42u16.into()),
            (43, "".into(), "blabla".into()),
        ];

        let ctx = ContextOwned::default();

        let encoded = encode_part_sheet_for_cache(part_sheet, &ctx.context())
            .expect("encoding should not fail");
        let decoded =
            decode_cached_part_sheet(&encoded, &ctx.context()).expect("decoding should not fail");

        let expected: &[(u32, Variant)] = &[
            (561, true.into()),
            (98, 42u16.into()),
            (43, "blabla".into()),
        ];

        assert_eq!(decoded, expected);
    }

    #[test]
    fn part_sheet_row_serialization() {
        let saved_at = "1984-12-09T04:30:54.123Z"
            .parse()
            .expect("parsing timestamp should not fail");
        let part_id = "123456789XX422611100001"
            .parse()
            .expect("parsing part identifier should not fail");
        let part_sheet: &[SavedPartSheetItem] = &[
            (0, "first".into(), true.into()),
            (0, "second".into(), 42u16.into()),
            (0, "third".into(), "blabla".into()),
        ];
        let row = PartSheetRow {
            saved_at,
            machine_id: "MAC1",
            part_id,
            data: part_sheet,
        };

        assert_ser_tokens(
            &row,
            &[
                Token::Struct {
                    name: "PartSheetRow",
                    len: 4,
                },
                Token::Str("saved_at"),
                Token::Str("1984-12-09 04:30:54.123"),
                Token::Str("machine_id"),
                Token::Str("MAC1"),
                Token::Str("part_id"),
                Token::Str("123456789XX422611100001"),
                Token::Str("data"),
                Token::Map {
                    len: Some(part_sheet.len()),
                },
                Token::Str("first"),
                Token::Bool(true),
                Token::Str("second"),
                Token::U16(42),
                Token::Str("third"),
                Token::Some,
                Token::Str("blabla"),
                Token::MapEnd,
                Token::StructEnd,
            ],
        );
    }
}
