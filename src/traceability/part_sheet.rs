use std::num::TryFromIntError;

use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use tracing::instrument;

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
pub(super) fn encode_cached_part_sheet<'a, I>(
    part_sheet: I,
    ctx: &Context,
) -> Result<Vec<u8>, TryFromIntError>
where
    I: IntoIterator<Item = (u32, &'a Variant)>,
    <I as IntoIterator>::IntoIter: Clone + ExactSizeIterator,
{
    let iterator = part_sheet.into_iter();

    let count: u16 = iterator.len().try_into()?;

    let encoded_elements_size = iterator
        .clone()
        .map(|(id, variant)| id.byte_len(ctx) + variant.byte_len(ctx))
        .sum::<usize>();

    let mut buf: Vec<u8> = Vec::with_capacity(count.byte_len(ctx) + encoded_elements_size);

    // Encode the number of elements.
    count
        .encode(&mut buf, ctx)
        .expect("writing to a Vec should not fail");

    for (id, variant) in iterator {
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

#[cfg(test)]
mod tests {
    use opcua::types::ContextOwned;

    use super::*;

    #[test]
    fn cache_encoding_roudtrip() {
        let fixture: [(u32, Variant); 3] = [
            (561, true.into()),
            (98, 42u16.into()),
            (43, "blabla".into()),
        ];

        let ctx = ContextOwned::default();

        let part_sheet_iter = fixture.iter().map(|(id, v)| (*id, v));
        let encoded = encode_cached_part_sheet(part_sheet_iter, &ctx.context())
            .expect("encoding should not fail");
        let decoded =
            decode_cached_part_sheet(&encoded, &ctx.context()).expect("decoding should not fail");

        assert_eq!(decoded, fixture);
    }
}
