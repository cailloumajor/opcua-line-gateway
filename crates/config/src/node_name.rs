use std::borrow::Borrow;
use std::error::Error;
use std::fmt;
use std::io::{Read, Write};

use opcua::types::{DecodingOptions, EncodingResult, SimpleBinaryDecodable, SimpleBinaryEncodable};
use schemars::JsonSchema;
use serde::Deserialize;

type SizeType = u8;

/// The error type returned when building a [`NodeName`] fails.
#[derive(Clone, Debug)]
pub struct NodeNameError(String);

impl fmt::Display for NodeNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "node name {} must not exceed {} bytes",
            self.0,
            SizeType::MAX
        )
    }
}

impl Error for NodeNameError {}

/// `BrowseName` of an OPC-UA node, whose length in bytes is guaranteed to fit in
/// an [`u8`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
pub struct NodeName(String);

impl NodeName {
    /// Get a string slice from this [`NodeName`].
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NodeName {
    type Error = NodeNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Ensure the length fits in an u8.
        if SizeType::try_from(value.len()).is_err() {
            return Err(NodeNameError(value));
        }

        Ok(Self(value))
    }
}

impl Borrow<str> for NodeName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for NodeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl SimpleBinaryEncodable for NodeName {
    fn byte_len(&self) -> usize {
        size_of::<SizeType>() + self.0.len()
    }

    fn encode<S: Write + ?Sized>(&self, stream: &mut S) -> EncodingResult<()> {
        // Guaranteed at construction.
        let len = self.0.len() as SizeType;
        len.encode(stream)?;
        stream
            .write_all(self.0.as_bytes())
            .map_err(opcua::types::Error::encoding)
    }
}

impl SimpleBinaryDecodable for NodeName {
    fn decode<S: Read + ?Sized>(
        stream: &mut S,
        decoding_options: &DecodingOptions,
    ) -> EncodingResult<Self> {
        let len = SizeType::decode(stream, decoding_options)?;

        let max = decoding_options.max_string_length;
        if usize::from(len) > max {
            return Err(opcua::types::Error::decoding(format!(
                "string buf length {len} exceeds decoding limit {max}"
            )));
        }

        let mut buf = vec![0u8; usize::from(len)];
        stream
            .read_exact(&mut buf)
            .map_err(opcua::types::Error::decoding)?;
        let inner = String::from_utf8(buf).map_err(|err| {
            opcua::types::Error::decoding(format!("decoded string is not valid UTF-8: {err}"))
        })?;

        Ok(Self(inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_error() {
        let long_string = (0..256).map(|_| ' ').collect::<String>();

        NodeName::try_from(long_string)
            .expect_err("creating a node name from a too long string should fail");
    }

    #[test]
    fn try_from_success() {
        let fitting_string = (0..255).map(|_| ' ').collect::<String>();

        let node_name = NodeName::try_from(fitting_string.clone())
            .expect("creating a node name from a short enough string should not fail");

        assert_eq!(node_name.as_str(), fitting_string);
    }

    #[test]
    fn encoding_byte_len() {
        let node_name =
            NodeName::try_from("a string".to_owned()).expect("creating node name should not fail");

        assert_eq!(node_name.byte_len(), 9);
    }

    #[test]
    fn encoding_roundtrip() {
        let node_name = NodeName::try_from("some string".to_owned())
            .expect("creating node name should not fail");

        let mut buf = [0u8; 12];
        node_name
            .encode(&mut buf.as_mut_slice())
            .expect("encoding node name should not fail");

        let decoded = NodeName::decode(&mut buf.as_slice(), &Default::default())
            .expect("decoding node name should not fail");

        assert_eq!(decoded, node_name);
    }

    #[test]
    fn decoding_limit_failure() {
        let buf = [4, b'a', b'b', b'c', b'd'];
        let options = DecodingOptions {
            max_string_length: 3,
            ..Default::default()
        };

        NodeName::decode(&mut buf.as_slice(), &options)
            .expect_err("decoding a string exceeding configured length should fail");
    }

    #[test]
    fn decoding_utf8_failure() {
        let buf = [5, b'a', b'b', 0xFF, b'c', b'd'];

        NodeName::decode(&mut buf.as_slice(), &Default::default())
            .expect_err("decoding an invalid UTF-8 string should fail");
    }
}
