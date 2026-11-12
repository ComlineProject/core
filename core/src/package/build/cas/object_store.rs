// Object store for content-addressable storage

use super::storage::{Hash, compress, decompress};
use eyre::{eyre, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// This binary's expected shape for everything persisted under `.comline/`
/// (`FrozenUnit`, `Blob`/`Tree`/`Commit`, ...) - **not** the package-authored
/// `specification_version` (that's the user's own chosen language edition,
/// unrelated to core's internal IR representation).
///
/// CBOR tags struct fields and enum variants by name, so growing or
/// reordering `FrozenUnit` needs no bump here - only *renaming* or
/// *removing* an existing field/variant does, since that's the one case a
/// self-describing format can't absorb for free. Bump this by hand when you
/// do one of those; [`ensure_compatible_store`] then wipes any `.comline/`
/// stamped with a different value instead of trying (and likely failing,
/// or in the worst case silently misreading) to decode it.
///
/// History:
/// - 1: CBOR, replacing bincode's position-based encoding.
/// - 2: `FrozenUnit::Property`'s `expression: Option<String>` became
///   `value: ConfigValue` - a field renamed *and* retyped, which is
///   exactly the case name tagging cannot absorb.
/// - 3: types became structured (`TypeRef`, `Literal`) instead of
///   `KindValue` text: `Field`/`Constant` `kind_value` became `ty` plus a
///   literal, `EnumVariant` holds its name, and `Assert` gained `expr`.
///
/// Additions that needed no bump: the root tree's `docs` blob (the `//!`
/// headers, `crate::doc::FrozenDocs`, core#145) is a new entry that older
/// readers skip and newer ones treat as absent on older versions. So are a
/// vendored dependency's `config` and `dependency_configs` blobs, in its
/// `dep_<name>` tree (core#251): a tree without them reads as having not
/// kept the dependency's config
/// ([`StoredConfigs`](crate::package::config::ir::frozen::cas::blob::StoredConfigs)),
/// and bumping this would have wiped every store's history to say so.
pub const CURRENT_IR_FORMAT_VERSION: u32 = 3;

fn format_version_path(project_root: &Path) -> PathBuf {
    project_root.join(".comline/FORMAT_VERSION")
}

/// Call before trusting anything already in `.comline/` (i.e. before
/// deciding whether a previous commit exists to diff against). A missing
/// or mismatched stamp - including every `.comline/` that predates this
/// stamp existing at all - means the store's content can't be trusted, so
/// it's wiped wholesale rather than partially trusted. This doesn't
/// conflict with the CAS's own "objects are never deleted" design
/// philosophy (see `cas/mod.rs`) - that's about normal operation within one
/// format; content in an incompatible format was never buildable to begin
/// with, so there's nothing of value to preserve.
pub fn ensure_compatible_store(project_root: &Path) -> Result<()> {
    let comline_dir = project_root.join(".comline");
    if !comline_dir.exists() {
        return Ok(()); // first build ever - nothing to check
    }

    let stored: Option<u32> = fs::read_to_string(format_version_path(project_root))
        .ok()
        .and_then(|s| s.trim().parse().ok());

    if stored != Some(CURRENT_IR_FORMAT_VERSION) {
        tracing::warn!(
            "`.comline/` predates or mismatches this build's IR format \
             (found {stored:?}, need {CURRENT_IR_FORMAT_VERSION}) - rebuilding from scratch"
        );
        fs::remove_dir_all(&comline_dir)?;
    }

    Ok(())
}

/// Object store manages content-addressable storage
pub struct ObjectStore {
    objects_dir: PathBuf,  // .comline/objects/
    /// Where a [`scratch`](ObjectStore::scratch) store keeps what it writes.
    scratch: Option<std::sync::Mutex<std::collections::HashMap<Hash, Vec<u8>>>>,
}

impl ObjectStore {
    /// Create a new object store
    pub fn new(project_root: &Path) -> Self {
        Self {
            objects_dir: project_root.join(".comline/objects"),
            scratch: None,
        }
    }

    /// A store that reads the project's objects but keeps what it writes in
    /// memory: for working out what a build would store without storing it.
    pub fn scratch(project_root: &Path) -> Self {
        Self { scratch: Some(Default::default()), ..Self::new(project_root) }
    }

    /// Initialize the store (create directories), stamping it with
    /// [`CURRENT_IR_FORMAT_VERSION`] if this is the first time.
    pub fn init(&self) -> Result<()> {
        if self.scratch.is_some() {
            return Ok(());
        }
        fs::create_dir_all(&self.objects_dir)?;
        let Some(comline_dir) = self.objects_dir.parent() else {
            return Ok(());
        };
        let stamp = comline_dir.join("FORMAT_VERSION");
        if !stamp.exists() {
            fs::write(&stamp, CURRENT_IR_FORMAT_VERSION.to_string())?;
        }
        Ok(())
    }

    /// Write content to store, returns hash
    pub fn write(&self, content: &[u8]) -> Result<Hash> {
        let hash = Hash::from_bytes(content);

        if let Some(scratch) = &self.scratch {
            if !self.exists(&hash) {
                scratch.lock().expect("scratch store").insert(hash, content.to_vec());
            }
            return Ok(hash);
        }

        // Check if already exists (deduplication)
        if self.exists(&hash) {
            tracing::debug!("Object {} already exists, skipping write", hash);
            return Ok(hash);
        }
        
        let compressed = compress(content);
        let path = self.objects_dir.join(hash.to_path());
        
        // Create parent directory (e.g., .comline/objects/ab/)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        tracing::trace!("Wrote object {} ({} byte(s) compressed)", hash, compressed.len());
        fs::write(&path, compressed)?;
        
        Ok(hash)
    }

    /// Read content from store by hash
    pub fn read(&self, hash: &Hash) -> Result<Vec<u8>> {
        if let Some(content) = self.scratch.as_ref().and_then(|s| s.lock().expect("scratch store").get(hash).cloned()) {
            return Ok(content);
        }
        let path = self.objects_dir.join(hash.to_path());
        
        if !path.exists() {
            return Err(eyre!("Object {} not found", hash));
        }
        
        let compressed = fs::read(path)?;
        let content = decompress(&compressed)?;
        
        // Verify hash matches
        let actual_hash = Hash::from_bytes(&content);
        if actual_hash != *hash {
            return Err(eyre!(
                "Hash mismatch: expected {}, got {}",
                hash,
                actual_hash
            ));
        }
        
        Ok(content)
    }

    /// Check if object exists
    pub fn exists(&self, hash: &Hash) -> bool {
        self.scratch.as_ref().is_some_and(|s| s.lock().expect("scratch store").contains_key(hash))
            || self.objects_dir.join(hash.to_path()).exists()
    }

    /// Get the path to an object (for debugging)
    pub fn object_path(&self, hash: &Hash) -> PathBuf {
        self.objects_dir.join(hash.to_path())
    }

    /// List all objects in the store (for debugging/GC)
    pub fn list_objects(&self) -> Result<Vec<Hash>> {
        let mut hashes = Vec::new();
        
        if !self.objects_dir.exists() {
            return Ok(hashes);
        }
        
        // Iterate through all subdirectories (ab/, cd/, etc.)
        for dir_entry in fs::read_dir(&self.objects_dir)? {
            let dir_entry = dir_entry?;
            let dir_path = dir_entry.path();
            
            if !dir_path.is_dir() {
                continue;
            }
            
            let dir_name = dir_entry.file_name().to_string_lossy().to_string();
            
            // Iterate through files in subdirectory
            for file_entry in fs::read_dir(&dir_path)? {
                let file_entry = file_entry?;
                let file_name = file_entry.file_name().to_string_lossy().to_string();
                
                // Reconstruct full hash (dir_name + file_name)
                let full_hex = format!("{}{}", dir_name, file_name);
                if let Ok(hash) = Hash::from_hex(&full_hex) {
                    hashes.push(hash);
                }
            }
        }
        
        Ok(hashes)
    }
}
