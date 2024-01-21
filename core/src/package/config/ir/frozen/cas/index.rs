// Standard Uses
use std::path::PathBuf;
use std::collections::HashMap;

// Crate Uses
use crate::project::ir::frozen::FrozenUnit;

// External Uses
use eyre::{Result, eyre};
use rmp_serde::Serializer;
use serde_derive::{Serialize, Deserialize};


#[derive(Serialize, Deserialize)]
pub struct Index {
    pub versions: HashMap<String, String>,
}
impl Index {
    pub fn from_project(path: &PathBuf) -> Result<Self> {
        let filename = path.file_name().unwrap().to_str().unwrap();
        let contents = std::fs::read(path)?;
        let hash = blake3::hash(&contents);

        if hash.to_string() != filename {
            panic!(
                "Project meta file name '{}' does not match its content's hash '{}'",
                filename, hash
            );
        }

        rmp_serde::from_slice(&contents).map_err(|e| eyre!(e))
    }

    pub fn to_processed(&self) -> (String, Vec<u8>) {
        let mut output = vec![];

        serde::Serialize::serialize(
            &self, &mut Serializer::new(&mut output)
        ).unwrap();
        let hash = blake3::hash(&output);

        (hash.to_string(), output)
    }
}


pub fn from_project_meta(path: &PathBuf) -> Result<Vec<FrozenUnit>> {
    let filename = path.file_name().unwrap().to_str().unwrap();
    let contents = std::fs::read(path)?;
    let hash = blake3::hash(&contents);

    if hash.to_string() != filename {
        panic!(
            "Project meta file name '{}' does not match its content's hash '{}'",
            filename, hash
        );
    }

    let deserialized: Vec<FrozenUnit> = rmp_serde::from_slice(&contents)?;

    Ok(deserialized)
}

pub fn to_processed(units: Vec<FrozenUnit>) -> (String, Vec<u8>) {
    let mut output = vec![];

    serde::Serialize::serialize(
        &units, &mut Serializer::new(&mut output)
    ).unwrap();
    let hash = blake3::hash(&output);

    (hash.to_string(), output)
}
