// CAS Object Types
// - Blob: Stores raw content
// - Tree: Organizes objects hierarchically
// - Commit: Tracks version history

use super::storage::Hash;
use eyre::{eyre, Result};
use serde_derive::{Deserialize, Serialize};

// ========== Blob ==========

/// Blob stores raw binary content (typically a serialized FrozenUnit)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blob {
    /// Raw content (not compressed - compression handled by ObjectStore)
    pub content: Vec<u8>,
}

impl Blob {
    /// Create a new blob from content
    pub fn new(content: Vec<u8>) -> Self {
        Self { content }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| eyre!("Failed to serialize blob: {}", e))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| eyre!("Failed to deserialize blob: {}", e))
    }

    /// Get the hash of this blob's content
    pub fn hash(&self) -> Hash {
        Hash::from_bytes(&self.content)
    }
}

// ========== Tree ==========

/// Tree organizes multiple entries (files/directories in a schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    pub mode: EntryMode,
    pub name: String,
    pub hash: Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryMode {
    Blob,   // Regular file (schema file)
    Tree,   // Directory (namespace)
}

impl Tree {
    /// Create a new empty tree
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Add an entry to the tree
    pub fn add_entry(&mut self, mode: EntryMode, name: String, hash: Hash) {
        self.entries.push(TreeEntry { mode, name, hash });
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| eyre!("Failed to serialize tree: {}", e))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| eyre!("Failed to deserialize tree: {}", e))
    }

    /// Get the hash of this tree
    pub fn hash(&self) -> Result<Hash> {
        let bytes = self.to_bytes()?;
        Ok(Hash::from_bytes(&bytes))
    }

    /// Find an entry by name
    pub fn find_entry(&self, name: &str) -> Option<&TreeEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

// ========== Commit ==========

/// Commit represents a version in the project history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// Root tree hash
    pub tree: Hash,
    
    /// Parent commit hashes (empty for initial commit)
    pub parents: Vec<Hash>,
    
    /// Author (username)
    pub author: String,
    
    /// Unix timestamp
    pub timestamp: i64,
    
    /// Commit message
    pub message: String,
    
    /// Semantic version (e.g., "0.1.0")
    pub version: String,
}

impl Commit {
    /// Create a new commit
    pub fn new(
        tree: Hash,
        parents: Vec<Hash>,
        author: String,
        timestamp: i64,
        message: String,
        version: String,
    ) -> Self {
        Self {
            tree,
            parents,
            author,
            timestamp,
            message,
            version,
        }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| eyre!("Failed to serialize commit: {}", e))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| eyre!("Failed to deserialize commit: {}", e))
    }

    /// Get the hash of this commit
    pub fn hash(&self) -> Result<Hash> {
        let bytes = self.to_bytes()?;
        Ok(Hash::from_bytes(&bytes))
    }

    /// Check if this is an initial commit (no parents)
    pub fn is_initial(&self) -> bool {
        self.parents.is_empty()
    }
}
