// Object store for content-addressable storage

use super::storage::{Hash, compress, decompress};
use eyre::{eyre, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Object store manages content-addressable storage
pub struct ObjectStore {
    objects_dir: PathBuf,  // .comline/objects/
}

impl ObjectStore {
    /// Create a new object store
    pub fn new(project_root: &Path) -> Self {
        Self {
            objects_dir: project_root.join(".comline/objects"),
        }
    }

    /// Initialize the store (create directories)
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.objects_dir)?;
        Ok(())
    }

    /// Write content to store, returns hash
    pub fn write(&self, content: &[u8]) -> Result<Hash> {
        let hash = Hash::from_bytes(content);
        
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
        
        tracing::trace!("Wrote object {} ({} bytes compressed)", hash, compressed.len());
        fs::write(&path, compressed)?;
        
        Ok(hash)
    }

    /// Read content from store by hash
    pub fn read(&self, hash: &Hash) -> Result<Vec<u8>> {
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
        self.objects_dir.join(hash.to_path()).exists()
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
