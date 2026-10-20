// CAS Object Types
// - Blob: Stores raw content
// - Tree: Organizes objects hierarchically
// - Commit: Tracks version history

use super::storage::{from_cbor, to_cbor, Hash};
use eyre::Result;
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
        to_cbor(self)
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        from_cbor(bytes)
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
        to_cbor(self)
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        from_cbor(bytes)
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
    
    /// Semantic version: a release (`0.3.0`), or a build between releases
    /// (`0.3.0-dev.2`, the release it leads to plus a count).
    pub version: String,

    /// Set on a commit `comline release` made: what was released and why.
    /// Absent on builds, and on releases made before release records
    /// existed (any version without a pre-release part is a release).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Release>,
}

/// What a release was: the bump its changes called for, the one applied,
/// and why they differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// The bump the changes since the previous release call for.
    pub computed: super::version::VersionBump,
    /// The bump applied: the computed one, or raised above it.
    pub applied: super::version::VersionBump,
    /// Why the bump was raised, or the package made stable.
    pub reason: Option<String>,
    /// A raise was asked for that the computed bump already reaches: the
    /// reason is kept as a note.
    pub covered: bool,
    /// This release moved the package to 1.0.0.
    pub stable: bool,
    /// The previous release's commit, if any: the changes behind this one
    /// are those between the two.
    pub since: Option<Hash>,
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
            release: None,
        }
    }

    /// Whether this commit is a release rather than a build between
    /// releases.
    pub fn is_release(&self) -> bool {
        self.release.is_some() || super::version::is_release(&self.version)
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        to_cbor(self)
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        from_cbor(bytes)
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
