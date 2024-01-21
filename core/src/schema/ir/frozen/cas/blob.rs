// Standard Uses

// Crate Uses

// External Uses
use lz4_flex;
use eyre::{bail, Result};


#[derive(Debug, PartialEq)]
pub enum FrozenBlob {
    Content(String)
}

/// Serializes a vector of frozen blobs into semi-human readable text suitable for storage
pub fn to_processed(nodes: Vec<FrozenBlob>) -> (String, Vec<u8>) {
    let mut content = String::new();

    for node in nodes {
        match node {
            FrozenBlob::Content(c) => {
                if !content.is_empty() {
                    panic!("There should not be more than one content node!")
                }
                content = c;
            }
        }
    }

    let blob = format!("blob {} {}", content.as_bytes().len(), content);

    // Since we have header + content size + content together, lets hash it
    let hash = blake3::hash(blob.as_bytes());

    let compressed_blob = lz4_flex::compress_prepend_size(blob.as_bytes());

    (hash.to_string(), compressed_blob)
}


pub fn from_processed(hash: String, processed: Vec<u8>) -> Result<Vec<FrozenBlob>> {
    let uncompressed = lz4_flex::decompress_size_prepended(&processed).unwrap();

    let processed_hash = blake3::hash(&uncompressed);

    if processed_hash.to_string() != hash {
        bail!(
            "Object hash is '{}' after decompressed, but expected {}",
            processed_hash, hash
        );
    }

    let raw = String::from_utf8(uncompressed)?;
    let (id, content) = raw.split_at("blob 10 ".len());
    let (_, size) = id.split_at("blob ".len());
    let size: usize = size.split_whitespace().next().unwrap().parse()?;

    if size != content.len() {
        bail!("Context size is '{}' but expected was '{}'", content.len(), size)
    }

    Ok(vec![FrozenBlob::Content(content.to_owned())])
}
