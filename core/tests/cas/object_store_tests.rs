use comline_core::package::build::cas::ObjectStore;
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
