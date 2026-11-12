use comline_core::package::build::cas::object_store::{
    ensure_compatible_store, StoreError, StoreState, CURRENT_IR_FORMAT_VERSION,
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
fn ensure_compatible_store_is_fresh_with_no_existing_comline_dir() {
    let temp_dir = TempDir::new().unwrap();
    // Nothing has ever built here - no `.comline/` at all.
    assert_eq!(ensure_compatible_store(temp_dir.path()), Ok(StoreState::Fresh));
    assert!(!temp_dir.path().join(".comline").exists());
}

#[test]
fn a_comline_dir_with_only_a_deps_cache_is_fresh() {
    // Dependencies are fetched into `.comline/deps-cache/` before the first
    // build, so a `.comline/` with no stamp and no history is not a store
    // of unknown format.
    let temp_dir = TempDir::new().unwrap();
    let cache = temp_dir.path().join(".comline/deps-cache/dep");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("file"), "fetched").unwrap();

    assert_eq!(ensure_compatible_store(temp_dir.path()), Ok(StoreState::Fresh));
    assert_eq!(fs::read_to_string(cache.join("file")).unwrap(), "fetched");

    // An empty `objects/` is no history either.
    fs::create_dir_all(temp_dir.path().join(".comline/objects")).unwrap();
    assert_eq!(ensure_compatible_store(temp_dir.path()), Ok(StoreState::Fresh));
}

#[test]
fn a_store_with_no_format_stamp_is_refused_and_left_alone() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    // Simulate a store built before this stamp existed at all.
    fs::remove_file(temp_dir.path().join(".comline/FORMAT_VERSION")).unwrap();
    let hash = store.write(b"stale content").unwrap();

    assert_eq!(ensure_compatible_store(temp_dir.path()), Err(StoreError::Unstamped));

    assert_eq!(store.read(&hash).unwrap(), b"stale content");
    assert!(!temp_dir.path().join(".comline/FORMAT_VERSION").exists());
}

#[test]
fn an_older_stamp_is_refused_and_left_alone() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    let hash = store.write(b"history").unwrap();
    let older = (CURRENT_IR_FORMAT_VERSION - 1).to_string();
    fs::write(temp_dir.path().join(".comline/FORMAT_VERSION"), &older).unwrap();

    assert_eq!(
        ensure_compatible_store(temp_dir.path()),
        Err(StoreError::Older { found: CURRENT_IR_FORMAT_VERSION - 1 })
    );

    assert_eq!(store.read(&hash).unwrap(), b"history");
    let stamp = fs::read_to_string(temp_dir.path().join(".comline/FORMAT_VERSION")).unwrap();
    assert_eq!(stamp, older);
}

#[test]
fn a_newer_stamp_is_refused_and_left_alone() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    let hash = store.write(b"history").unwrap();
    fs::write(temp_dir.path().join(".comline/FORMAT_VERSION"), "999999\n").unwrap();

    assert_eq!(
        ensure_compatible_store(temp_dir.path()),
        Err(StoreError::Newer { found: 999999 })
    );

    assert_eq!(store.read(&hash).unwrap(), b"history");
    let stamp = fs::read_to_string(temp_dir.path().join(".comline/FORMAT_VERSION")).unwrap();
    assert_eq!(stamp, "999999\n");
}

#[test]
fn a_stamp_that_is_not_a_number_is_refused() {
    for text in ["", "three", "-1", "+3", "3.0", "1.0/0", "99999999999999999999", "3 3"] {
        let temp_dir = TempDir::new().unwrap();
        let store = ObjectStore::new(temp_dir.path());
        store.init().unwrap();
        fs::write(temp_dir.path().join(".comline/FORMAT_VERSION"), text).unwrap();

        assert_eq!(
            ensure_compatible_store(temp_dir.path()),
            Err(StoreError::InvalidStamp { text: text.to_string() }),
            "stamp {text:?}"
        );
    }
}

#[test]
fn an_unreadable_stamp_is_refused() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    // A directory where the file should be: reading it fails, and it isn't
    // "not found", so this is not "unstamped".
    let stamp = temp_dir.path().join(".comline/FORMAT_VERSION");
    fs::remove_file(&stamp).unwrap();
    fs::create_dir(&stamp).unwrap();

    assert!(matches!(
        ensure_compatible_store(temp_dir.path()),
        Err(StoreError::UnreadableStamp { .. })
    ));
    assert!(stamp.is_dir());
}

#[test]
fn init_refuses_what_ensure_compatible_store_refuses_and_stamps_nothing() {
    let temp_dir = TempDir::new().unwrap();
    // History with no stamp: `init` must not stamp it as current.
    let objects = temp_dir.path().join(".comline/objects/ab");
    fs::create_dir_all(&objects).unwrap();
    fs::write(objects.join("cdef"), "x").unwrap();

    let error = ObjectStore::new(temp_dir.path()).init().unwrap_err();
    assert_eq!(error.downcast_ref::<StoreError>(), Some(&StoreError::Unstamped));
    assert!(!temp_dir.path().join(".comline/FORMAT_VERSION").exists());
}

#[test]
fn the_messages_say_what_was_found_that_nothing_changed_and_what_to_do() {
    let current = CURRENT_IR_FORMAT_VERSION;
    let older = StoreError::Older { found: 2 }.to_string();
    assert!(older.contains("IR format 2"), "{older}");
    assert!(older.contains(&format!("format {current}")), "{older}");
    assert!(older.contains("Nothing was changed"), "{older}");
    assert!(older.contains("`comline store reset`"), "{older}");
    // That command is named but doesn't exist yet, and the text says so.
    assert!(older.contains("not in this release yet"), "{older}");

    let newer = StoreError::Newer { found: 9 }.to_string();
    assert!(newer.contains("IR format 9"), "{newer}");
    assert!(newer.contains("a newer comline wrote"), "{newer}");
    assert!(newer.contains("Nothing was changed and nothing will be"), "{newer}");
    assert!(!newer.contains("reset"), "a newer store is never reset: {newer}");

    for error in [
        StoreError::Unstamped,
        StoreError::InvalidStamp { text: "x".into() },
        StoreError::UnreadableStamp { reason: "denied".into() },
    ] {
        let message = error.to_string();
        assert!(message.contains("FORMAT_VERSION"), "{message}");
        assert!(message.contains("Nothing was changed"), "{message}");
        assert!(message.contains("`comline store reset`"), "{message}");
    }
}

#[test]
fn ensure_compatible_store_leaves_a_matching_store_alone() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();
    let hash = store.write(b"kept content").unwrap();

    assert_eq!(ensure_compatible_store(temp_dir.path()), Ok(StoreState::Current));

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
