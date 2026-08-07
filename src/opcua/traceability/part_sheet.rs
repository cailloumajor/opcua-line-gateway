use std::io::{self, Cursor, Read};

use leb128::write::unsigned_len;
use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use redb::{TypeName, Value};
use tracing::instrument;

/// Represents a part sheet (a collection of [`Variant`]s), encoded to be cached with redb.
#[derive(Debug)]
pub(super) struct CachedPartSheet(Vec<u8>);

impl CachedPartSheet {
    /// Create an [`CachedPartSheet`], provided an iterator of node identifier and [`Variant`].
    #[instrument(skip_all)]
    pub(super) fn encode<'a, I>(pairs: I, ctx: &Context) -> Self
    where
        I: IntoIterator<Item = (u32, &'a Variant)>,
    {
        let pairs = pairs.into_iter().collect::<Vec<_>>();

        let count: u64 = pairs
            .len()
            .try_into()
            .expect("provided elements length should fit in an u64");

        // Compute the length in bytes of the encoded elements.
        let exact_len: usize = pairs
            .iter()
            .map(|(id, v)| size_of_val(id) + v.byte_len(ctx))
            .sum();

        let mut buf = Vec::with_capacity(unsigned_len(count) + exact_len);

        // Encode the number of elements (unsigned LEB128).
        leb128::write::unsigned(&mut buf, count).expect("writing to a Vec should not fail");

        for (id, variant) in pairs {
            // Encode the node identifier (32 bits little endian).
            buf.extend_from_slice(&id.to_le_bytes());
            // Encode the variant (OPC-UA binary encoding).
            variant
                .encode(&mut buf, ctx)
                .expect("writing to a Vec should not fail");
        }

        Self(buf)
    }

    /// Consume and decode this [`CachedPartSheet`], returning a vector of pairs of [`Variant`]
    /// and node identifier.
    #[instrument(err, skip_all)]
    pub(super) fn decode(&self, ctx: &Context) -> io::Result<Vec<(u32, Variant)>> {
        let mut cursor = Cursor::new(self.0.as_slice());

        // Decode the number of elements (unsigned LEB128).
        let count = leb128::read::unsigned(&mut cursor).map_err(|e| match e {
            leb128::read::Error::IoError(err) => err,
            leb128::read::Error::Overflow => io::Error::other(e),
        })?;

        let mut out = Vec::new();

        for _ in 0..count {
            // Decode the node identifier (32 bits little endian).
            let mut id_bytes = [0u8; 4];
            cursor.read_exact(&mut id_bytes)?;
            let id = u32::from_le_bytes(id_bytes);
            // Decode the variant (OPC-UA binary encoding).
            let variant = Variant::decode(&mut cursor, ctx)?;

            out.push((id, variant));
        }

        Ok(out)
    }
}

impl Value for CachedPartSheet {
    type SelfType<'a>
        = CachedPartSheet
    where
        Self: 'a;

    type AsBytes<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        Self(data.to_vec())
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        &value.0
    }

    fn type_name() -> TypeName {
        TypeName::new("opcua_line_gateway::CachedPartSheet")
    }
}

#[cfg(test)]
mod tests {
    use opcua::types::{ContextOwned, VariantScalarTypeId};

    use super::*;

    #[test]
    fn encode() {
        let identifiers = [561, 98, 43];
        let variants = &[true.into(), 42u16.into(), "blabla".into()];
        let pairs = identifiers.into_iter().zip(variants);
        let ctx = ContextOwned::default();

        let encoded = CachedPartSheet::encode(pairs, &ctx.context());

        #[rustfmt::skip]
        let expected: &[u8] = &[
            // Length (unsigned LEB128).
            3,
            // First element node identifier (32-bit little endian).
            0x31, 0x02, 0x00, 0x00,
            // First element encoding mask.
            VariantScalarTypeId::Boolean.encoding_mask(),
            // First element value (true).
            1,
            // Second element node identifier (32-bit little endian).
            98, 0, 0, 0,
            // Second element encoding mask.
            VariantScalarTypeId::UInt16.encoding_mask(),
            // Second element value (little endian).
            42, 0,
            // Third element node identifier (32-bit little endian).
            43, 0, 0, 0,
            // Third element encoding mask.
            VariantScalarTypeId::String.encoding_mask(),
            // Third element length (32-bit little endian).
            6, 0, 0, 0,
            // Third element value.
            b'b', b'l', b'a', b'b', b'l', b'a',
        ];

        assert_eq!(encoded.0, expected);
    }

    #[test]
    fn decode() {
        #[rustfmt::skip]
        let encoded: Vec<u8> = vec![
            // Length (unsigned LEB128).
            3,
            // First element node identifier (32-bit little endian).
            0x31, 0x02, 0x00, 0x00,
            // First element encoding mask.
            VariantScalarTypeId::Boolean.encoding_mask(),
            // First element value (true).
            1,
            // Second element node identifier (32-bit little endian).
            98, 0, 0, 0,
            // Second element encoding mask.
            VariantScalarTypeId::UInt16.encoding_mask(),
            // Second element value (little endian).
            42, 0,
            // Third element node identifier (32-bit little endian).
            43, 0, 0, 0,
            // Third element encoding mask.
            VariantScalarTypeId::String.encoding_mask(),
            // Third element length (32-bit little endian).
            6, 0, 0, 0,
            // Third element value.
            b'b', b'l', b'a', b'b', b'l', b'a',
        ];
        let ctx = ContextOwned::default();

        let decoded = CachedPartSheet(encoded)
            .decode(&ctx.context())
            .expect("decoding should not fail");

        let expected = &[
            (561, true.into()),
            (98, 42u16.into()),
            (43, "blabla".into()),
        ];

        assert_eq!(decoded, expected);
    }
}
