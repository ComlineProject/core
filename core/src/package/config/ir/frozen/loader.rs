// Standard Uses
use std::fs;
use std::path::Path;

// Crate Uses
use crate::package::config::ir::frozen::{
    FrozenUnit as ProjectUnit,
    basic_storage as basic_storage_project
};
use crate::schema::ir::frozen::{
    unit::FrozenUnit as SchemaUnit,
    basic_storage as basic_storage_schema
};

// External Uses
use eyre::{Context, Result};


pub type PackageVersion = (Vec<ProjectUnit>, Vec<Vec<SchemaUnit>>);

pub fn load_package(package_path: &Path) -> Result<Vec<PackageVersion>> {
    let frozen_path = package_path.join(".frozen/");
    let versions_path = frozen_path.join("package/versions/");

    let mut versions = vec![];
    for entry in fs::read_dir(versions_path)? {
        let entry = entry?.path();

        if !entry.is_dir() { panic!("") }

        let version_path = entry.as_path();
        versions.push(load_package_version(version_path)?);
    }

    Ok(versions)
}

fn load_package_version(version_path: &Path) -> Result<PackageVersion> {
    let config = basic_storage_project::deserialize::all_from_origin(
        version_path
    )?;

    let schemas_path = Path::new(version_path).join("schemas/");

    let mut schemas = vec![];
    let dir = fs::read_dir(&schemas_path)
        .with_context(|| format!("Couldn't read entries at path: '{}'", schemas_path.display()))?;

    for entry in dir {
        let entry = entry?.path();

        if !entry.is_dir() {
            panic!("")
        }

        let version_path = entry.as_path();
        schemas.push(
            basic_storage_schema::deserialize::from_schema_frozen(version_path)?
        );
    }

    Ok((config, schemas))
}
