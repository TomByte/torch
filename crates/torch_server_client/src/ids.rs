use itertools::Itertools;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const SERVER_ID_LENGTH: usize = 22;

/// ServerId is a representation of a string-based unique ID we generate on the server,
/// of length SERVER_ID_LENGTH.
/// Because it's of fixed length, it can implement the Copy trait
/// (in contrast to simply using a String type).
#[derive(Clone, Copy, Default, Hash, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(description = "A server-assigned unique identifier.")]
pub struct ServerId([char; SERVER_ID_LENGTH]);

/// UID for API keys.
pub type ApiKeyUid = String;

#[derive(Debug, thiserror::Error)]
pub enum ParseServerIdError {
    #[error("ServerId must be exactly {SERVER_ID_LENGTH} characters, got {len}")]
    InvalidLength { len: usize },
}

impl ServerId {
    /// Convert a string input to a server ID. If the string is not exactly
    /// [`SERVER_ID_LENGTH`] characters long, it will be truncated or padded as
    /// necessary.
    pub fn from_string_lossy(id: impl AsRef<str>) -> Self {
        let id = id.as_ref();
        Self::try_from(id).unwrap_or_else(|err| {
            if cfg!(debug_assertions) {
                panic!("{err}");
            }
            // ServerIds need to be exactly 22 characters, so to prevent a crash, we'll normalize
            // the string. Nothing that uses it will work, but it's better than crashing.
            let normalized = Self::normalize_id_str(id, 0);
            Self::try_from(normalized).expect("id should convert")
        })
    }

    /// Normalizes a string to be exactly 22 characters long.
    fn normalize_id_str(input: &str, prefix_length: usize) -> String {
        let available_len = SERVER_ID_LENGTH - prefix_length;
        let truncated = if input.len() > available_len {
            &input[input.len() - available_len..]
        } else {
            input
        };
        format!("{truncated:0>available_len$}")
    }

    pub fn uid(&self) -> String {
        (*self).into()
    }
}

impl TryFrom<&str> for ServerId {
    type Error = ParseServerIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.chars().collect_array() {
            Some(chars) => Ok(Self(chars)),
            None => Err(ParseServerIdError::InvalidLength {
                len: s.chars().count(),
            }),
        }
    }
}

impl TryFrom<String> for ServerId {
    type Error = ParseServerIdError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::try_from(id.as_str())
    }
}

/// Creates a conversion between an i64 and a corresponding deterministic ServerId for use in tests.
/// An i64 like 123 will be converted to "test_uid00000000000123".
#[cfg(any(test, feature = "test-util"))]
impl From<i64> for ServerId {
    fn from(id: i64) -> Self {
        let prefix = "test_uid";
        let id_str = id.abs().to_string();
        let normalized = format!(
            "{}{}",
            prefix,
            Self::normalize_id_str(&id_str, prefix.len())
        );
        Self::try_from(normalized).expect("normalized string should always be valid")
    }
}

impl From<ServerId> for String {
    fn from(id: ServerId) -> String {
        String::from_iter(id.0)
    }
}

/// We need our own implementation of serialize, due to ServerId being essentially a char array.
/// The default serializer in this case would spit a string that looks like an array, instead of a
/// nicely formatted string that we want. This implementation would serialize ServerId('a', 'b') to
/// "ab" instead.
impl Serialize for ServerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s: String = (*self).into();
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for ServerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        ServerId::try_from(s.as_str()).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use std::fmt::Write;
        for ch in self.0.iter() {
            f.write_char(*ch)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "ServerId({self})")
    }
}
