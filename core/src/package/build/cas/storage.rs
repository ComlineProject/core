// Core storage primitives for CAS
// - Blake3 hashing for content addressing
// - LZ4 compression for space efficiency

use blake3;
use eyre::{eyre, Result};
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// 256-bit Blake3 hash for content addressing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    /// Create a hash from raw bytes
    pub fn from_bytes(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Hash(*hash.as_bytes())
    }

    /// Create hash from hex string
    pub fn from_hex(hex: &str) -> Result<Self> {
        if hex.len() != 64 {
            return Err(eyre!("Invalid hex length: expected 64, got {}", hex.len()));
        }

        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| eyre!("Invalid hex: {}", e))?;
        }
        Ok(Hash(bytes))
    }

    /// Convert to hex string (64 characters)
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Convert to filesystem path (e.g., "ab/cdef...")
    /// First 2 chars become directory, rest is filename
    pub fn to_path(&self) -> PathBuf {
        let hex = self.to_hex();
        PathBuf::from(&hex[0..2]).join(&hex[2..])
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Compress data using lz4_flex
pub fn compress(data: &[u8]) -> Vec<u8> {
    lz4_flex::compress_prepend_size(data)
}

/// Decompress data using lz4_flex
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    lz4_flex::decompress_size_prepended(data)
        .map_err(|e| eyre!("Decompression failed: {}", e))
}

/// Encode a CAS-persisted value as CBOR - tags struct fields and enum
/// variants by name, not declaration-order position, unlike `bincode`
/// (which this replaced). That's what lets `FrozenUnit` grow/reorder
/// safely without corrupting anything already on disk; only a rename or
/// removal of an existing field/variant still needs
/// `cas::CURRENT_IR_FORMAT_VERSION` bumped.
pub fn to_cbor<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf).map_err(|e| eyre!("CBOR encode failed: {}", e))?;
    Ok(buf)
}

/// Decode a CAS-persisted CBOR value. See [`to_cbor`].
pub fn from_cbor<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::de::from_reader(bytes).map_err(|e| eyre!("CBOR decode failed: {}", e))
}
