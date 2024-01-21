// Relative Modules
pub(crate) mod serialize;
pub mod deserialize;

// Standard Uses
use std::path::Path;

// Crate Uses

// External Uses
use semver::Version;


pub fn get_latest_version(frozen_path: &Path) -> Option<Version> {
    let package_path = frozen_path.join("package/");
    if !package_path.exists() { return None }

    let index_path = package_path.join("index");
    if !index_path.exists() { return None }

    let index = std::fs::read_to_string(index_path).unwrap();
    let index = index.split('\n').collect::<Vec<_>>();

    index.last().map(|idx| Version::parse(idx).unwrap())
}

pub fn has_any_frozen_content(package_path: &Path) -> bool {
    let frozen_path = package_path.join(".frozen");
    if !frozen_path.exists() { return false }

    let package_path = frozen_path.join("package/");
    if !package_path.exists() { return false }

    let index_path = package_path.join("index");
    if !index_path.exists() { return false }

    // TODO: Even if the 'index' file doesn't exist, the versions directory
    //       still might have content, now decide if we consider this a full disposal.
    //       It might not matter since Basic Storage is a temporary development feature
    //       to be substituted by the CAS, which doesn't fail as much on integrity

    let index = std::fs::read_to_string(index_path).unwrap();
    let index = index.split('\n').collect::<Vec<_>>();

    index.len() > 0
}

