// Content Addressable Storage (CAS) System
// 
// Git-inspired object storage for immutable versioning of schemas.
// Objects are stored by their blake3 hash with lz4 compression.

// Modules
pub mod storage;       // Hash type and compression utilities
pub mod object_store;  // CAS operations (read/write)
pub mod objects;       // Blob, Tree, Commit types
pub mod refs;          // Git-style references management
pub mod build;         // Build process implementation
mod package;
mod schema;

// Re-exports
pub use storage::{Hash, compress, decompress};
pub use object_store::ObjectStore;
pub use refs::{update_ref, read_ref, ref_exists, main_ref};

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;

// External Uses
use eyre::Result;

/// Process changes using CAS - delegates to build module
#[allow(unused)]
pub fn process_changes(
    project_path: &Path,
    latest_project: &ProjectContext
) -> Result<()> {
    build::process_changes(project_path, latest_project)?;
    Ok(())
}

/// Process initial freezing using CAS - delegates to build module
#[allow(unused)]
pub fn process_initial_freezing(
    project_path: &Path,
    latest_project: &ProjectContext
) -> Result<()> {
    build::process_initial_freezing(project_path, latest_project)?;
    Ok(())
}
