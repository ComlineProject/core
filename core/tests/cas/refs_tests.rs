// Tests for refs management

use comline_core::package::build::cas::{Hash, read_ref, ref_exists, update_ref};
use tempfile::TempDir;

#[test]
fn test_update_and_read_ref() {
    let temp_dir = TempDir::new().unwrap();
    let hash = Hash::from_bytes(b"test commit");

    update_ref(temp_dir.path(), "refs/heads/main", &hash).unwrap();

    let read_hash = read_ref(temp_dir.path(), "refs/heads/main").unwrap();
    assert_eq!(hash, read_hash);
}

#[test]
fn test_ref_exists() {
    let temp_dir = TempDir::new().unwrap();

    assert!(!ref_exists(temp_dir.path(), "refs/heads/main"));

    let hash = Hash::from_bytes(b"test");
    update_ref(temp_dir.path(), "refs/heads/main", &hash).unwrap();

    assert!(ref_exists(temp_dir.path(), "refs/heads/main"));
}

#[test]
fn test_read_nonexistent_ref() {
    let temp_dir = TempDir::new().unwrap();

    let result = read_ref(temp_dir.path(), "refs/heads/nonexistent");
    assert!(result.is_err());
}
