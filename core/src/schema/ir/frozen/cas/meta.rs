// Standard Uses
use std::path::PathBuf;

// Crate Uses
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses
use eyre::Result;


pub fn from_schema_meta(path: &PathBuf) -> Result<Vec<FrozenUnit>> {
    let filename = path.file_name().unwrap().to_str().unwrap();
    let contents = std::fs::read(path)?;
    let hash = blake3::hash(&contents);

    if hash.to_string() != filename {
        panic!(
            "Meta file name '{}' does not match its content's hash '{}'",
            filename, hash
        );
    }

    let deserialized: Vec<FrozenUnit> = rmp_serde::from_slice(&contents)?;

    Ok(deserialized)
}

#[allow(unused)]
pub fn to_schema_meta(frozen: &[FrozenUnit]) -> Result<String> {
    todo!()
}

