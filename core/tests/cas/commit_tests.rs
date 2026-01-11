// Tests for commit creation (commit.rs)

use comline_core::package::build::cas::Hash;
use comline_core::schema::ir::frozen::cas::commit::{
    create_initial_commit, create_version_commit, get_timestamp,
};

#[test]
fn test_create_initial_commit() {
    let tree_hash = Hash::from_bytes(b"test_tree");
    let commit = create_initial_commit(tree_hash, "0.0.1");

    assert_eq!(commit.version, "0.0.1");
    assert_eq!(commit.message, "Initial commit");
    assert!(commit.is_initial());
    assert_eq!(commit.tree, tree_hash);
}

#[test]
fn test_create_version_commit() {
    let tree_hash = Hash::from_bytes(b"new_tree");
    let parent_hash = Hash::from_bytes(b"parent_commit");

    let commit = create_version_commit(tree_hash, parent_hash, "1.0.0", "Major version bump");

    assert_eq!(commit.version, "1.0.0");
    assert_eq!(commit.message, "Major version bump");
    assert!(!commit.is_initial());
    assert_eq!(commit.parents.len(), 1);
    assert_eq!(commit.parents[0], parent_hash);
}

#[test]
fn test_author_is_set() {
    let tree_hash = Hash::from_bytes(b"test");
    let commit = create_initial_commit(tree_hash, "0.0.1");

    assert!(!commit.author.is_empty());
}

#[test]
fn test_timestamp_is_reasonable() {
    let tree_hash = Hash::from_bytes(b"test");
    let commit = create_initial_commit(tree_hash, "0.0.1");

    // Timestamp should be recent (within last year)
    let now = get_timestamp();
    let year_in_seconds = 365 * 24 * 60 * 60;

    assert!(commit.timestamp > now - year_in_seconds);
    assert!(commit.timestamp <= now);
}
