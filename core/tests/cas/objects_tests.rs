use comline_core::package::build::cas::objects::{Blob, Tree, TreeEntry, EntryMode, Commit};
use comline_core::package::build::cas::Hash;

#[test]
fn test_blob_roundtrip() {
    let content = b"test content".to_vec();
    let blob = Blob::new(content.clone());
    
    let bytes = blob.to_bytes().unwrap();
    let restored = Blob::from_bytes(&bytes).unwrap();
    
    assert_eq!(blob.content, restored.content);
}

#[test]
fn test_blob_hash() {
    let blob1 = Blob::new(b"same content".to_vec());
    let blob2 = Blob::new(b"same content".to_vec());
    let blob3 = Blob::new(b"different content".to_vec());
    
    assert_eq!(blob1.hash(), blob2.hash());
    assert_ne!(blob1.hash(), blob3.hash());
}

#[test]
fn test_tree_operations() {
    let mut tree = Tree::new();
    let hash1 = Hash::from_bytes(b"content1");
    let hash2 = Hash::from_bytes(b"content2");
    
    tree.add_entry(EntryMode::Blob, "file1.ids".to_string(), hash1);
    tree.add_entry(EntryMode::Tree, "src/".to_string(), hash2);
    
    assert_eq!(tree.entries.len(), 2);
    assert!(tree.find_entry("file1.ids").is_some());
    assert!(tree.find_entry("nonexistent").is_none());
}

#[test]
fn test_tree_roundtrip() {
    let mut tree = Tree::new();
    tree.add_entry(
        EntryMode::Blob,
        "test.ids".to_string(),
        Hash::from_bytes(b"test"),
    );
    
    let bytes = tree.to_bytes().unwrap();
    let restored = Tree::from_bytes(&bytes).unwrap();
    
    assert_eq!(tree.entries.len(), restored.entries.len());
    assert_eq!(tree.entries[0].name, restored.entries[0].name);
}

#[test]
fn test_commit_creation() {
    let tree_hash = Hash::from_bytes(b"tree");
    let commit = Commit::new(
        tree_hash,
        vec![],
        "test-user".to_string(),
        1704902400,
        "Initial commit".to_string(),
        "0.0.1".to_string(),
    );
    
    assert!(commit.is_initial());
    assert_eq!(commit.version, "0.0.1");
}

#[test]
fn test_commit_roundtrip() {
    let commit = Commit::new(
        Hash::from_bytes(b"tree"),
        vec![Hash::from_bytes(b"parent")],
        "user".to_string(),
        1704902400,
        "Test commit".to_string(),
        "1.0.0".to_string(),
    );
    
    let bytes = commit.to_bytes().unwrap();
    let restored = Commit::from_bytes(&bytes).unwrap();
    
    assert_eq!(commit.version, restored.version);
    assert_eq!(commit.message, restored.message);
    assert_eq!(commit.parents.len(), restored.parents.len());
}
