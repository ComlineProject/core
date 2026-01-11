// References management for CAS (like Git refs)

use crate::package::build::cas::Hash;
use eyre::{eyre, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Update a reference (e.g., refs/heads/main) to point to a commit
pub fn update_ref(project_path: &Path, ref_name: &str, commit_hash: &Hash) -> Result<()> {
    let refs_path = project_path.join(".comline").join(ref_name);
    
    // Create parent directories
    if let Some(parent) = refs_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    // Write commit hash to ref file
    fs::write(&refs_path, commit_hash.to_hex())?;
    
    tracing::debug!("Updated ref {} to {}", ref_name, commit_hash);
    Ok(())
}

/// Read a reference and get the commit hash it points to
pub fn read_ref(project_path: &Path, ref_name: &str) -> Result<Hash> {
    let refs_path = project_path.join(".comline").join(ref_name);
    
    if !refs_path.exists() {
        return Err(eyre!("Ref {} does not exist", ref_name));
    }
    
    let hex = fs::read_to_string(&refs_path)?;
    Hash::from_hex(hex.trim())
}

/// Check if a reference exists
pub fn ref_exists(project_path: &Path, ref_name: &str) -> bool {
    project_path.join(".comline").join(ref_name).exists()
}

/// Get the main branch ref path
pub fn main_ref() -> &'static str {
    "refs/heads/main"
}
