use comline_core::package::build::cas::object_store::{
    ensure_compatible_store, CURRENT_IR_FORMAT_VERSION,
};
use comline_core::package::build::cas::ObjectStore;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_object_store_write_read() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let content = b"Hello, CAS!";
    let hash = store.write(content).unwrap();

    let retrieved = store.read(&hash).unwrap();
    assert_eq!(content.as_slice(), retrieved.as_slice());
}

#[test]
fn test_deduplication() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let content = b"duplicate content";
    let hash1 = store.write(content).unwrap();
    let hash2 = store.write(content).unwrap();

    assert_eq!(hash1, hash2);
    assert!(store.exists(&hash1));
}

#[test]
fn test_hash_verification() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let content = b"verify this";
    let hash = store.write(content).unwrap();

    // Content should match hash
    let retrieved = store.read(&hash).unwrap();
    use comline_core::package::build::cas::Hash;
    assert_eq!(Hash::from_bytes(&retrieved), hash);
}

#[test]
fn ensure_compatible_store_is_a_noop_with_no_existing_comline_dir() {
    let temp_dir = TempDir::new().unwrap();
    // Nothing has ever built here - no `.comline/` at all.
    ensure_compatible_store(temp_dir.path()).unwrap();
    assert!(!temp_dir.path().join(".comline").exists());
}

#[test]
fn ensure_compatible_store_wipes_a_store_with_no_format_stamp() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    // Simulate a store built before this stamp existed at all.
    fs::remove_file(temp_dir.path().join(".comline/FORMAT_VERSION")).unwrap();
    store.write(b"stale content").unwrap();

    ensure_compatible_store(temp_dir.path()).unwrap();

    assert!(!temp_dir.path().join(".comline").exists());
}

#[test]
fn ensure_compatible_store_wipes_a_store_with_a_mismatched_stamp() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    fs::write(temp_dir.path().join(".comline/FORMAT_VERSION"), "999999").unwrap();

    ensure_compatible_store(temp_dir.path()).unwrap();

    assert!(!temp_dir.path().join(".comline").exists());
}

#[test]
fn ensure_compatible_store_leaves_a_matching_store_alone() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    let hash = store.write(b"kept content").unwrap();

    ensure_compatible_store(temp_dir.path()).unwrap();

    assert!(temp_dir.path().join(".comline").exists());
    assert_eq!(store.read(&hash).unwrap(), b"kept content");
}

#[test]
fn init_stamps_a_fresh_store_with_the_current_format_version() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let stamp = fs::read_to_string(temp_dir.path().join(".comline/FORMAT_VERSION")).unwrap();
    assert_eq!(stamp, CURRENT_IR_FORMAT_VERSION.to_string());
}

#[test]
fn test_list_objects() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let hashes = vec![
        store.write(b"object 1").unwrap(),
        store.write(b"object 2").unwrap(),
        store.write(b"object 3").unwrap(),
    ];

    let listed = store.list_objects().unwrap();
    assert_eq!(listed.len(), 3);
    
    for hash in hashes {
        assert!(listed.contains(&hash));
    }
}
