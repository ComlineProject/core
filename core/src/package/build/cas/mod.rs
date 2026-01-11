// Content Addressable Storage (CAS) for Comline
// Git-inspired immutable storage for schema versioning
//
// CAS Design Philosophy: Append-Only Immutable Storage
//
// Comline's CAS is designed for permanent, linear versioning:
// - Commits form an unbroken chain via parent links
// - Objects are NEVER deleted or garbage collected  
// - All versions remain buildable forever
// - No branches, force pushes, or history rewriting
//
// This ensures 100% reproducible builds for any historical version.
// Every commit is reachable by traversing backwards from refs/heads/main.

// Moduleschemas.
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
