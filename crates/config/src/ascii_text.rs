use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

/// Errors that can occur with [`AsciiText`].
#[derive(Debug, Error)]
pub enum AsciiTextError {
    #[error("input length {0} is not expected length {1}")]
    BadLength(usize, usize),
    #[error("non-printable ASCII byte 0x{0:02X} at position {1}")]
    NonPrintable(u8, usize),
}

/// A fixed-size, immutable ASCII string.
#[derive(Clone, Copy, Debug)]
pub struct AsciiText<const LENGTH: usize>([u8; LENGTH]);

impl<const LENGTH: usize> TryFrom<&[u8]> for AsciiText<LENGTH> {
    type Error = AsciiTextError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != LENGTH {
            return Err(AsciiTextError::BadLength(value.len(), LENGTH));
        }

        if let Some((pos, byte)) = value
            .iter()
            .enumerate()
            .find(|(_, b)| !b.is_ascii_graphic())
        {
            return Err(AsciiTextError::NonPrintable(*byte, pos));
        }

        let inner = value
            .try_into()
            .expect("converting slice to array should not fail");

        Ok(Self(inner))
    }
}

impl<const LENGTH: usize> FromStr for AsciiText<LENGTH> {
    type Err = AsciiTextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.as_bytes().try_into()
    }
}

impl<const LENGTH: usize> fmt::Display for AsciiText<LENGTH> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = str::from_utf8(&self.0).expect("converting ASCII to UTF-8 should not fail");
        f.write_str(s)
    }
}

impl<'de, const LENGTH: usize> Deserialize<'de> for AsciiText<LENGTH> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AsciiTextVisitor<const LENGTH: usize>;

        impl<const LENGTH: usize> Visitor<'_> for AsciiTextVisitor<LENGTH> {
            type Value = AsciiText<LENGTH>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    formatter,
                    "a printable ASCII string with {LENGTH} characters"
                )
            }

            #[inline]
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.try_into().map_err(E::custom)
            }

            #[inline]
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(AsciiTextVisitor)
    }
}

impl<const LENGTH: usize> JsonSchema for AsciiText<LENGTH> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "AsciiText".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": LENGTH,
            "maxLength": LENGTH,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn too_short() {
        let result = "ABC".parse::<AsciiText<4>>();

        assert_matches!(result, Err(AsciiTextError::BadLength(3, 4)));
    }

    #[test]
    fn too_long() {
        let result = "ABCDE".parse::<AsciiText<4>>();

        assert_matches!(result, Err(AsciiTextError::BadLength(5, 4)));
    }

    #[test]
    fn non_ascii() {
        let result = "AB\tD".parse::<AsciiText<4>>();

        assert_matches!(result, Err(AsciiTextError::NonPrintable(0x09, 2)));
    }

    #[test]
    fn okay() {
        let ascii_text = "R2D2"
            .parse::<AsciiText<4>>()
            .expect("parsing should not fail");

        assert_eq!(ascii_text.to_string(), "R2D2");
    }
}
