use std::ffi::OsString;
use std::fmt::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::constants::{HEX_DIGITS_PER_BYTE, HEX_RADIX, INVALID_HEX_MESSAGE};

/// NAME-11: a path as JSON. Linux allows any bytes in names, so a path that is not UTF-8
/// travels as `{"hex": "…"}`, its bytes in lower-case hexadecimal; every other path is a plain
/// string, as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonPath {
    Text(String),
    Bytes { hex: String },
}

impl JsonPath {
    pub fn of(path: &Path) -> Self {
        match path.to_str() {
            Some(text) => Self::Text(text.to_owned()),
            None => Self::Bytes {
                hex: encode_hex(path.as_os_str().as_bytes()),
            },
        }
    }

    /// None when the hexadecimal does not read.
    pub fn into_path(self) -> Option<PathBuf> {
        match self {
            Self::Text(text) => Some(PathBuf::from(text)),
            Self::Bytes { hex } => {
                decode_hex(&hex).map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
            }
        }
    }
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u32> = text
        .chars()
        .map(|digit| digit.to_digit(HEX_RADIX))
        .collect::<Option<_>>()?;
    if !digits.len().is_multiple_of(HEX_DIGITS_PER_BYTE) {
        return None;
    }
    digits
        .chunks(HEX_DIGITS_PER_BYTE)
        .map(|pair| {
            let value = pair
                .iter()
                .fold(0, |value, digit| value * HEX_RADIX + digit);
            u8::try_from(value).ok()
        })
        .collect()
}

/// For a stored path, such as the history's: `#[serde(with = "crate::json_path::stored")]`.
pub mod stored {
    use super::*;

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        JsonPath::of(path).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        JsonPath::deserialize(deserializer)?
            .into_path()
            .ok_or_else(|| D::Error::custom(INVALID_HEX_MESSAGE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// NAME-11: a UTF-8 path stays a string, and any other comes back byte for byte.
    #[test]
    fn hex_paths_round_trip() {
        let legacy = Path::new(OsStr::from_bytes(b"/tmp/Fat\xe3.srt"));
        let as_json = JsonPath::of(legacy);
        assert_eq!(
            serde_json::to_string(&as_json).unwrap(),
            r#"{"hex":"2f746d702f466174e32e737274"}"#
        );
        assert_eq!(as_json.into_path().as_deref(), Some(legacy));
        let plain: JsonPath = serde_json::from_str(r#""/tmp/Fată.srt""#).unwrap();
        assert_eq!(plain.into_path(), Some(PathBuf::from("/tmp/Fată.srt")));
        for bad in ["2f7", "zz", "+f", "2f 7"] {
            let path = JsonPath::Bytes {
                hex: String::from(bad),
            };
            assert_eq!(path.into_path(), None, "{bad}");
        }
    }
}
