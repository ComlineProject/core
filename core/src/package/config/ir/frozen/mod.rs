// Relative Module
pub mod basic_storage;
// pub(crate) mod cas;
pub mod loader;

// Standard Uses
use std::slice::Iter;
use std::iter::FilterMap;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;

// External Uses
use serde_derive::{Serialize, Deserialize};


pub type FrozenWhole = (ProjectContext, Vec<FrozenUnit>);


#[derive(Debug, Eq, PartialEq, Clone)]
#[derive(Deserialize, Serialize)]
pub enum FrozenUnit {
    Namespace(String),
    SpecificationVersion(u8),
    PackageVersion(String),
    SchemaPath(String),
    Dependency(Dependency),
    CodeGeneration(LanguageDetails),
    PublishRegistry((String, PublishRegistry))
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Dependency {
    pub(crate) author: String,
    pub(crate) project: String,
    pub(crate) version: String
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LanguageDetails {
    pub(crate) name: String,
    pub(crate) versions: Vec<String>,
    pub(crate) generation_path: Option<String>
}

// TODO: Optimization might be possible here, and for other structures
//       advised in https://discord.com/channels/619623572318453784/737119153282089109/1158659223534702674
#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PublishRegistry {
    pub kind: RegistryKind,
    pub uri: String,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum RegistryKind {
    LocalStorage, RegistryServer
}

pub const MINIMUM_VERSION: &str = "0.0.1";


pub fn namespace(units: &[FrozenUnit]) -> Option<&str> {
    for unit in units {
        if let FrozenUnit::Namespace(namespace) = unit {
            return Some(namespace)
        }
    }

    None
}

pub fn version(units: &[FrozenUnit]) -> Option<&str> {
    for unit in units {
        if let FrozenUnit::PackageVersion(version) = unit {
            return Some(version)
        }
    }

    None
}

pub fn dependencies(units: &[FrozenUnit]) -> Vec<&Dependency> {
    let mut deps = vec![];

    for unit in units {
        if let FrozenUnit::Dependency(dep) = unit {
            deps.push(dep)
        }
    }

    deps
}

#[allow(clippy::type_complexity)]
pub fn schema_paths(units: &[FrozenUnit])
    -> FilterMap<Iter<'_, FrozenUnit>, fn(&FrozenUnit) -> Option<&str>>
{
    units.iter().filter_map(|unit| {
        match unit {
            FrozenUnit::SchemaPath(path) => Some(path),
            _ => None
        }
    })
}

pub fn publish_registries(
    units: &[FrozenUnit]
) -> FilterMap<Iter<'_, FrozenUnit>, fn(&FrozenUnit) -> Option<&(String, PublishRegistry)>>
{
    units.iter().filter_map(|unit| {
        match unit {
            FrozenUnit::PublishRegistry(reg) => Some(reg),
            _ => None
        }
    })
}

