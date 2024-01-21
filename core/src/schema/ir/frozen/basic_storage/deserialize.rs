// Standard Uses
use std::path::Path;

// Crate Uses
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses
use eyre::{Result, eyre};


/// Load all schema frozen units from a specific version's frozen schemas directory
/// # Arguments
/// * `version_path` - Path to a package's frozen version
/// # Example
/// ```
/// use std::path::Path;
/// use comline_core::schema::ir::frozen::basic_storage;
///
/// let froze_version_path = Path::new("some_package/.frozen/0.0.1/");
/// let frozen_version = basic_storage::deserialize::all_from_version_frozen(froze_version_path);
/// ```
pub fn all_from_version_frozen(version_path: &Path) -> Result<Vec<Vec<FrozenUnit>>> {
    let schemas_path = version_path.join("schemas/");

    let mut schemas = vec![];

    let ent = glob::glob(&*format!("{}/**/*", schemas_path.display()))?;
    for result in ent {
        let entry = result?;
        if entry.is_dir() { continue }

        let frozen_schema = from_schema_frozen(&entry)?;
        schemas.push(frozen_schema);
    }

    /*
    let entries = std::fs::read_dir(&schemas_path)
        .with_context(||
            format!("Couldn't read version frozen data at '{}'", schemas_path.display())
        )?;

    for file in entries {
        let schema_path = file?.path();

        if !schema_path.is_file() {
            panic!(
                "Expected a frozen schema file but got a directory or something else instead at '{:?}'",
                &schema_path
            )
        }

        schemas.push(from_schema_frozen(&schema_path)?);
    }
    */

    Ok(schemas)
}

pub(crate) fn from_schema_frozen(schema_path: &Path) -> Result<Vec<FrozenUnit>> {
    let schema = std::fs::read(schema_path)?;
    let schema_frozen  = rmp_serde::from_slice(&schema)
        .map_err(|e| eyre!(e));

    schema_frozen
}

