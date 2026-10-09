use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// Errors that can occur with [`AsciiDigitsOrUpper`].
#[derive(Debug, Error)]
pub enum AsciiDigitsOrUpperError {
    #[error("input length {0} is not expected length {1}")]
    BadLength(usize, usize),
    #[error("non digit or uppercase ASCII character `{char}` at position {1}", char = .0.escape_ascii())]
    InvalidChar(u8, usize),
}

/// A fixed-size, immutable string made only of ASCII digits and uppercase letters.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct AsciiDigitsOrUpper<const LENGTH: usize>([u8; LENGTH]);

impl<const LENGTH: usize> AsciiDigitsOrUpper<LENGTH> {
    /// Gets a reference to the underlying array.
    pub const fn as_array(&self) -> &[u8; LENGTH] {
        &self.0
    }

    /// Return the string as a string slice.
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.0).expect("converting ASCII to UTF-8 should not fail")
    }
}

impl<const LENGTH: usize> TryFrom<&[u8]> for AsciiDigitsOrUpper<LENGTH> {
    type Error = AsciiDigitsOrUpperError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let inner: [u8; LENGTH] = value
            .try_into()
            .map_err(|_| AsciiDigitsOrUpperError::BadLength(value.len(), LENGTH))?;

        if let Some(pos) = inner
            .iter()
            .position(|b| !b.is_ascii_digit() && !b.is_ascii_uppercase())
        {
            return Err(AsciiDigitsOrUpperError::InvalidChar(inner[pos], pos + 1));
        }

        Ok(Self(inner))
    }
}

impl<const LENGTH: usize> TryFrom<String> for AsciiDigitsOrUpper<LENGTH> {
    type Error = AsciiDigitsOrUpperError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl<const LENGTH: usize> FromStr for AsciiDigitsOrUpper<LENGTH> {
    type Err = AsciiDigitsOrUpperError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.as_bytes().try_into()
    }
}

impl<const LENGTH: usize> fmt::Display for AsciiDigitsOrUpper<LENGTH> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const LENGTH: usize> Serialize for AsciiDigitsOrUpper<LENGTH> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<const LENGTH: usize> JsonSchema for AsciiDigitsOrUpper<LENGTH> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "AsciiDigitsOrUpper".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": LENGTH,
            "maxLength": LENGTH,
            "pattern": format!("^[0-9A-Z]{{{LENGTH}}}$"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn too_short() {
        let result = "ABC".parse::<AsciiDigitsOrUpper<4>>();

        assert_matches!(result, Err(AsciiDigitsOrUpperError::BadLength(3, 4)));
    }

    #[test]
    fn too_long() {
        let result = "ABCDE".parse::<AsciiDigitsOrUpper<4>>();

        assert_matches!(result, Err(AsciiDigitsOrUpperError::BadLength(5, 4)));
    }

    #[test]
    fn non_printable() {
        let result = "AB\tD".parse::<AsciiDigitsOrUpper<4>>();

        assert_matches!(result, Err(AsciiDigitsOrUpperError::InvalidChar(b'\t', 3)));
    }

    #[test]
    fn non_digit_or_uppercase() {
        let result = "ABCd".parse::<AsciiDigitsOrUpper<4>>();

        assert_matches!(result, Err(AsciiDigitsOrUpperError::InvalidChar(b'd', 4)));
    }

    #[test]
    fn okay() {
        let ascii_text = "R2D2"
            .parse::<AsciiDigitsOrUpper<4>>()
            .expect("parsing should not fail");

        assert_eq!(ascii_text.to_string(), "R2D2");
    }
}
