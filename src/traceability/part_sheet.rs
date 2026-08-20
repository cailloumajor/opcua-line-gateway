use std::num::TryFromIntError;
use std::vec::IntoIter;

use opcua::types::{BinaryDecodable, BinaryEncodable, Context, Variant};
use tracing::instrument;

/// Part sheet element type.
type PartSheetItem = (u32, Variant);

/// Represents a traceability part sheet, i.e. a collection of triples of node
/// identifiers, BrowseNames, and values (as [`Variant`]), to be used for caching.
pub(super) struct CachedPartSheet(Vec<PartSheetItem>);

impl FromIterator<PartSheetItem> for CachedPartSheet {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = PartSheetItem>,
    {
        Self(FromIterator::from_iter(iter))
    }
}

impl IntoIterator for CachedPartSheet {
    type Item = PartSheetItem;
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl CachedPartSheet {
    /// Return a reference to a variant at a provided position.
    pub(super) fn get_variant(&self, index: usize) -> Option<&Variant> {
        self.0.get(index).map(|(_, v)| v)
    }

    /// Decode a [`PartSheet`] from cache encoding format.
    ///
    /// # Errors
    ///
    /// Return an error if something goes wrong during decoding.
    #[instrument(err, skip_all)]
    pub(super) fn from_cache_encoding(mut buf: &[u8], ctx: &Context) -> std::io::Result<Self> {
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

        Ok(Self(out))
    }

    /// Encode this [`PartSheet`] to the format used for caching, returning the encoded bytes.
    ///
    /// # Errors
    ///
    /// Return an error if the number of elements is too big (must fit in an [`u16`]).
    #[instrument(skip_all)]
    pub(super) fn encode_for_cache(&self, ctx: &Context) -> Result<Vec<u8>, TryFromIntError> {
        let count: u16 = self.0.len().try_into()?;

        let encoded_elements_size = self
            .0
            .iter()
            .map(|(id, variant)| id.byte_len(ctx) + variant.byte_len(ctx))
            .sum::<usize>();

        let mut buf: Vec<u8> = Vec::with_capacity(count.byte_len(ctx) + encoded_elements_size);

        // Encode the number of elements.
        count
            .encode(&mut buf, ctx)
            .expect("writing to a Vec should not fail");

        for (id, variant) in self.0.iter() {
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
}

#[cfg(test)]
mod tests {
    use opcua::types::ContextOwned;

    use super::*;

    #[test]
    fn cache_encoding_roudtrip() {
        let fixture: [PartSheetItem; 3] = [
            (561, true.into()),
            (98, 42u16.into()),
            (43, "blabla".into()),
        ];

        let ctx = ContextOwned::default();

        let encoded = CachedPartSheet::from_iter(fixture.clone())
            .encode_for_cache(&ctx.context())
            .expect("encoding should not fail");
        let decoded = CachedPartSheet::from_cache_encoding(&encoded, &ctx.context())
            .expect("decoding should not fail");

        assert_eq!(decoded.0, fixture);
    }
}
