// Commit creation utilities for CAS

use crate::package::build::cas::objects::Commit;
use crate::package::build::cas::Hash;
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a commit for a schema version
pub fn create_commit(
    tree_hash: Hash,
    parent: Option<Hash>,
    version: &str,
    message: &str,
) -> Commit {
    Commit::new(
        tree_hash,
        parent.into_iter().collect(),
        get_author(),
        get_timestamp(),
        message.to_string(),
        version.to_string(),
    )
}

/// Create an initial commit (no parent)
pub fn create_initial_commit(tree_hash: Hash, version: &str) -> Commit {
    create_commit(tree_hash, None, version, "Initial commit")
}

/// Create a commit with a parent (version bump)
pub fn create_version_commit(
    tree_hash: Hash,
    parent_hash: Hash,
    version: &str,
    message: &str,
) -> Commit {
    create_commit(tree_hash, Some(parent_hash), version, message)
}

/// Get the current user as commit author
fn get_author() -> String {
    // Use whoami crate if available, otherwise default
    #[cfg(feature = "whoami")]
    {
        whoami::username()
    }
    #[cfg(not(feature = "whoami"))]
    {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}

/// Get current Unix timestamp
pub fn get_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}
