use opcua::types::Variant;
use serde::{Serialize, Serializer, ser};

/// Wrapper around a [`Variant`] reference, allowing serialization.
pub(crate) struct SerializeVariant<'a>(pub(crate) &'a Variant);

impl Serialize for SerializeVariant<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Variant::Empty => serializer.serialize_unit(),
            Variant::Boolean(v) => serializer.serialize_bool(*v),
            Variant::SByte(v) => serializer.serialize_i8(*v),
            Variant::Byte(v) => serializer.serialize_u8(*v),
            Variant::Int16(v) => serializer.serialize_i16(*v),
            Variant::UInt16(v) => serializer.serialize_u16(*v),
            Variant::Int32(v) => serializer.serialize_i32(*v),
            Variant::UInt32(v) => serializer.serialize_u32(*v),
            Variant::Int64(v) => serializer.serialize_i64(*v),
            Variant::UInt64(v) => serializer.serialize_u64(*v),
            Variant::Float(v) => serializer.serialize_f32(*v),
            Variant::Double(v) => serializer.serialize_f64(*v),
            Variant::String(v) => v.value().serialize(serializer),
            _ => Err(ser::Error::custom(format!(
                "unsupported serialization for Variant type {:?}",
                self.0.type_id()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use opcua::types::UAString;
    use serde_test::{Token, assert_ser_tokens};

    use super::*;

    #[test]
    fn serialize_supported() {
        let variants: [Variant; _] = [
            Variant::Empty,
            true.into(),             // Boolean
            (-126i8).into(),         // SByte
            42u8.into(),             // Byte
            (-32321i16).into(),      // Int16
            4521u16.into(),          // UInt16
            (-123456789i32).into(),  // Int32
            123456789u32.into(),     // UInt32
            (-9876543210i64).into(), // Int64
            9876543210u64.into(),    // UInt64
            3.15f32.into(),          // Float
            2.418281828f64.into(),   // Double
            UAString::null().into(), // Null string
            "hello opcua".into(),    // String
        ];
        let serializable = variants.iter().map(SerializeVariant).collect::<Vec<_>>();

        assert_ser_tokens(
            &serializable,
            &[
                Token::Seq {
                    len: Some(variants.len()),
                },
                Token::Unit,
                Token::Bool(true),
                Token::I8(-126),
                Token::U8(42),
                Token::I16(-32321),
                Token::U16(4521),
                Token::I32(-123456789),
                Token::U32(123456789),
                Token::I64(-9876543210),
                Token::U64(9876543210),
                Token::F32(3.15),
                Token::F64(2.418281828),
                Token::None,
                Token::Some,
                Token::Str("hello opcua"),
                Token::SeqEnd,
            ],
        );
    }
}
