// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::ir::frozen::FrozenUnit;

// External Uses
use eyre::{bail, Result};


pub fn from_latest_frozen(frozen_path: &Path) -> Result<Vec<FrozenUnit>> {
    let package_path = frozen_path.join("package/");
    if !package_path.exists() { bail!("Frozen package path doesn't exist") }

    let index_path = package_path.join("index");
    if !index_path.exists() { bail!("Frozen package 'index' file doesn't exist") }

    let index = std::fs::read_to_string(index_path).unwrap();
    let versions = index.split("\n").collect::<Vec<_>>();

    if versions.len() < 1 { bail!("No frozen versions are declared on index") }

    let latest_version = super::get_latest_version(&frozen_path)
        .unwrap();

    let versions_path = package_path.join("versions/");
    if !versions_path.exists() { bail!("Frozen package versions path doesn't exist") }

    let latest_version_path = versions_path.join(latest_version.to_string());
    if !latest_version_path.exists() {
        bail!("No latest version directory exists at {:?}", latest_version_path)
    }

    all_from_origin(&latest_version_path)
}

#[allow(unused)]
/// Load all config frozen units from a frozen origin projects directory
pub fn all_from_origin(version_path: &Path) -> Result<Vec<FrozenUnit>> {
    let config_path = version_path.join("config");
    if !config_path.exists() {
        bail!(
            "Frozen package versions 'config' file doesn't exist at '{}'", config_path.display()
        )
    }

    let config = std::fs::read(config_path)?;
    let units: Vec<FrozenUnit> = rmp_serde::from_slice(&config)?;

    Ok(units)
}

