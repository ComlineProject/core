//! Format 3's bytes: the one place that encodes and decodes them.
//!
//! Every payload is CBOR, name-tagged by serde. A reader here never guesses:
//! bytes that don't decode, or that carry what this copy of the format
//! doesn't know, are an error and not an empty value.

use std::fmt;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::types::{ConfigUnit, FrozenDocs, History, ProducedBy, SchemaUnit, UnknownRecord};

/// Bytes that are not a value of the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    what: &'static str,
    reason: String,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a format 3 {}: {}", self.what, self.reason)
    }
}

impl std::error::Error for CodecError {}

/// `value` as CBOR.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).map_err(|e| CodecError {
        what: "value",
        reason: e.to_string(),
    })?;
    Ok(bytes)
}

/// A `T` from CBOR, naming `what` it was meant to be when it isn't one.
pub fn decode<T: DeserializeOwned>(what: &'static str, bytes: &[u8]) -> Result<T, CodecError> {
    ciborium::de::from_reader(bytes).map_err(|e| CodecError {
        what,
        reason: e.to_string(),
    })
}

/// A schema unit's blob content.
pub fn decode_unit(bytes: &[u8]) -> Result<SchemaUnit, CodecError> {
    decode("schema unit", bytes)
}

/// A `config` blob's content: a version's, or a vendored dependency's.
pub fn decode_config(bytes: &[u8]) -> Result<Vec<ConfigUnit>, CodecError> {
    decode("config", bytes)
}

/// A `docs` blob's content.
pub fn decode_docs(bytes: &[u8]) -> Result<FrozenDocs, CodecError> {
    decode("docs", bytes)
}

/// A `facts` blob's content.
pub fn decode_history(bytes: &[u8]) -> Result<History, CodecError> {
    decode("facts", bytes)
}

/// An `unknown` blob's content. Fails on a record with a field this copy of
/// the format doesn't have: such a version is refused, not read short.
pub fn decode_unknown(bytes: &[u8]) -> Result<Vec<UnknownRecord>, CodecError> {
    decode("unknown list", bytes)
}

/// A `produced_by` blob's content, with the same refusal.
pub fn decode_produced_by(bytes: &[u8]) -> Result<ProducedBy, CodecError> {
    decode("produced_by record", bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> UnknownRecord {
        UnknownRecord {
            path: "/schemas/0/1/ty".into(),
            step: "3-4".into(),
            reason: "format 3 froze `f32` as `float`".into(),
            original: Some(b"float".to_vec()),
        }
    }

    #[test]
    fn an_unknown_list_round_trips() {
        let list = vec![
            record(),
            UnknownRecord {
                original: None,
                ..record()
            },
        ];
        assert_eq!(decode_unknown(&encode(&list).unwrap()).unwrap(), list);
        assert_eq!(
            decode_unknown(&encode(&Vec::<UnknownRecord>::new()).unwrap()).unwrap(),
            vec![]
        );
    }

    #[test]
    fn an_unknown_record_with_a_field_this_copy_lacks_is_refused() {
        #[derive(Serialize)]
        struct Newer {
            path: String,
            step: String,
            reason: String,
            confidence: u8,
        }
        let bytes = encode(&vec![Newer {
            path: "/x".into(),
            step: "3-4".into(),
            reason: "r".into(),
            confidence: 1,
        }])
        .unwrap();
        assert!(decode_unknown(&bytes).is_err());
    }

    #[test]
    fn bytes_that_are_not_an_unknown_list_are_an_error_not_an_empty_one() {
        for bytes in [&b""[..], &b"\xff"[..], &b"\x01"[..], &b"\xa0"[..]] {
            assert!(decode_unknown(bytes).is_err(), "{bytes:?}");
        }
    }

    #[test]
    fn produced_by_round_trips_and_refuses_what_it_does_not_know() {
        let record = ProducedBy {
            core: "0.1.0".into(),
            spec: "0.11".into(),
        };
        assert_eq!(
            decode_produced_by(&encode(&record).unwrap()).unwrap(),
            record
        );

        #[derive(Serialize)]
        struct Newer {
            core: String,
            spec: String,
            toolchain: String,
        }
        let newer = Newer {
            core: "0.2.0".into(),
            spec: "0.12".into(),
            toolchain: "1.99".into(),
        };
        let error = decode_produced_by(&encode(&newer).unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("not a format 3 produced_by record"),
            "{error}"
        );
        assert!(decode_produced_by(b"").is_err());
    }
}
