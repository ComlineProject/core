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
#[cfg(feature = "compiler")]
pub mod refs;          // Git-style references management
#[cfg(feature = "compiler")]
pub mod build;         // Build process implementation
pub mod version;       // Version bump types
#[cfg(feature = "compiler")]
pub mod release;       // Releases on top of builds
#[cfg(feature = "compiler")]
mod package;
#[cfg(feature = "compiler")]
mod schema;

// Re-exports
pub use storage::{Hash, compress, decompress};
pub use object_store::ObjectStore;
#[cfg(feature = "compiler")]
pub use refs::{update_ref, read_ref, ref_exists, main_ref};
pub use version::VersionBump;

// Standard Uses
#[cfg(feature = "compiler")]
use std::path::Path;

// Crate Uses
#[cfg(feature = "compiler")]
use crate::package::config::ir::context::ProjectContext;

// External Uses
#[cfg(feature = "compiler")]
use eyre::Result;

/// Process changes using CAS - delegates to build module
#[allow(unused)]
#[cfg(feature = "compiler")]
pub fn process_changes(
    project_path: &Path,
    latest_project: &ProjectContext
) -> Result<()> {
    build::process_changes(project_path, latest_project)?;
    Ok(())
}

/// Process initial freezing using CAS - delegates to build module
#[allow(unused)]
#[cfg(feature = "compiler")]
pub fn process_initial_freezing(
    project_path: &Path,
    latest_project: &ProjectContext
) -> Result<()> {
    build::process_initial_freezing(project_path, latest_project)?;
    Ok(())
}
