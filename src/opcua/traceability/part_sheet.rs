use std::io::{self, Cursor, Read};

use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use redb::{TypeName, Value};
use tracing::instrument;

/// Represents a part sheet (a collection of [`Variant`]s), encoded to be cached with redb.
#[derive(Debug)]
pub(super) struct CachedPartSheet(Vec<u8>);

impl CachedPartSheet {
    /// Create an [`CachedPartSheet`], provided an iterator of [`Variant`].
    pub(super) fn encode<'a, I>(pairs: I, ctx: &Context) -> Self
    where
        I: IntoIterator<Item = &'a Variant>,
    {
        let pairs = pairs.into_iter().collect::<Vec<_>>();

        assert!(
            pairs.len() <= u16::MAX as usize,
            "there should not be more than {} variants",
            u16::MAX
        );
        let count = pairs.len() as u16;

        let exact_len: usize = pairs.iter().map(|v| v.byte_len(ctx)).sum();

        let mut buf = Vec::with_capacity(size_of_val(&count) + exact_len);

        // Encode the number of elements (2 bytes little endian).
        buf.extend_from_slice(&count.to_le_bytes());

        for variant in pairs {
            // Encode the variant (OPC-UA binary encoding).
            variant
                .encode(&mut buf, ctx)
                .expect("writing to a Vec should not fail");
        }

        Self(buf)
    }

    /// Consume and decode this [`CachedPartSheet`], returning a vector of [`Variant`].
    #[instrument(err, skip_all)]
    pub(super) fn decode(&self, ctx: &Context) -> io::Result<Vec<Variant>> {
        let mut cursor = Cursor::new(self.0.as_slice());

        // Decode the number of elements (2 bytes little endian).
        let mut count_bytes = [0u8; 2];
        cursor.read_exact(&mut count_bytes)?;
        let count = u16::from_le_bytes(count_bytes);

        let mut out = Vec::new();

        for _ in 0..count {
            // Decode the variant (OPC-UA binary encoding).
            let variant = Variant::decode(&mut cursor, ctx)?;

            out.push(variant);
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
        let variants = vec![true.into(), 42u16.into(), "blabla".into()];
        let ctx = ContextOwned::default();

        let encoded = CachedPartSheet::encode(&variants, &ctx.context());

        #[rustfmt::skip]
        let expected: &[u8] = &[
            // Length (16-bit little endian).
            3, 0,
            // First element encoding mask.
            VariantScalarTypeId::Boolean.encoding_mask(),
            // First element value (true).
            1,
            // Second element encoding mask.
            VariantScalarTypeId::UInt16.encoding_mask(),
            // Second element value (little endian).
            42, 0,
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
            // Length (16-bit little endian).
            3, 0,
            // First element encoding mask.
            VariantScalarTypeId::Boolean.encoding_mask(),
            // First element value (true).
            1,
            // Second element encoding mask.
            VariantScalarTypeId::UInt16.encoding_mask(),
            // Second element value (little endian).
            42, 0,
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

        assert_eq!(decoded, &[true.into(), 42u16.into(), "blabla".into()]);
    }
}
