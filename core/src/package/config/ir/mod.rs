// Relative Modules
pub mod compiler;
pub mod interpreter;
pub mod context;
pub mod frozen;
pub mod diff;

// REMOVED: package_from_path_without_context()
// This function loaded data from .frozen/ directory which is replaced by CAS.
// The equivalent functionality with CAS would be:
// 1. Read refs/heads/main to get latest commit
// 2. Load commit object to get tree hash
// 3. Load tree and  blobs to reconstruct schema
//
// Currently this is not needed as the build() function in package/build/mod.rs
// handles all freezing/loading via CAS automatically.
