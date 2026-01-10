use comline_core::package::build::cas::storage::{Hash, compress, decompress};

#[test]
fn test_hash_from_bytes() {
    let data = b"hello world";
    let hash = Hash::from_bytes(data);
    
    // Blake3 of "hello world" is deterministic
    let hex = hash.to_hex();
    assert_eq!(hex.len(), 64);
}

#[test]
fn test_hash_roundtrip() {
    let data = b"test data";
    let hash1 = Hash::from_bytes(data);
    let hex = hash1.to_hex();
    let hash2 = Hash::from_hex(&hex).unwrap();
    
    assert_eq!(hash1, hash2);
}

#[test]
fn test_hash_to_path() {
    let hash = Hash::from_hex("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890").unwrap();
    let path = hash.to_path();
    
    assert_eq!(path.to_str().unwrap(), "ab/cdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
}

#[test]
fn test_compress_decompress() {
    let original = b"This is test data that should compress well. ".repeat(10);
    let compressed = compress(&original);
    let decompressed = decompress(&compressed).unwrap();
    
    assert_eq!(original.as_slice(), decompressed.as_slice());
    assert!(compressed.len() < original.len()); // Should be smaller
}

#[test]
fn test_hash_consistency() {
    let data = b"consistent data";
    let hash1 = Hash::from_bytes(data);
    let hash2 = Hash::from_bytes(data);
    
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.to_hex(), hash2.to_hex());
}
