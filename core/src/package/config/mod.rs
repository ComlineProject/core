// Relative Modules
pub mod idl;
pub mod ir;
#[cfg(feature = "compiler")]
pub mod dependency;  // NEW: Dependency configuration

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::idl::PACKAGE_CONFIG_FILENAME;

// External Uses


/// Determines if given path is a package directory by checking minimally
/// required files in it
pub fn is_package_path(package_path: &Path) -> bool {
    package_path.join(PACKAGE_CONFIG_FILENAME).exists()
}

