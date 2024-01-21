// Relative Modules
pub mod idl;
pub mod ir;

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::idl::constants::CONGREGATION_EXTENSION;

// External Uses


/// Determines if given path is a package directory by checking minimally
/// required files in it
pub fn is_package_path(package_path: &Path) -> bool {
    let config_path = package_path.join(
        format!("config.{}", CONGREGATION_EXTENSION)
    );

    config_path.exists()
}

