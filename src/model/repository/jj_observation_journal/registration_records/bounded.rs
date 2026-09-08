use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

#[derive(PartialEq, Eq)]
pub(crate) struct ByteString<const MAX: usize>(pub(crate) Vec<u8>);

impl<const MAX: usize> Serialize for ByteString<MAX> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for ByteString<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<const MAX: usize>;

        impl<'de, const MAX: usize> de::Visitor<'de> for Visitor<MAX> {
            type Value = ByteString<MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a nonempty byte string of at most {MAX} bytes")
            }

            fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
                if bytes.is_empty() || bytes.len() > MAX {
                    return Err(E::custom("registration byte string length invalid"));
                }
                Ok(ByteString(bytes))
            }
        }

        // The shared framing precheck bounds the complete record before this
        // owned allocation. Borrowed bytes would impose Ciborium's 4 KiB scratch limit.
        deserializer.deserialize_byte_buf(Visitor::<MAX>)
    }
}
