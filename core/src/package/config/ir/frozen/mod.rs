// Relative Modules
pub mod cas;
pub mod loader;

// Standard Uses
use std::slice::Iter;
use std::iter::FilterMap;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;

// External Uses
use serde_derive::{Serialize, Deserialize};


pub type FrozenWhole = (ProjectContext, Vec<FrozenUnit>);


/// Persisted into `.comline/` as CBOR, same as `schema::ir::frozen::unit::FrozenUnit`
/// (a separate enum of the same name, one level up in this module tree) -
/// see that type's own doc comment for what that means for safely
/// growing/reordering this one.
#[derive(Debug, Eq, PartialEq, Clone)]
#[derive(Deserialize, Serialize)]
pub enum FrozenUnit {
    Namespace(String),
    SpecificationVersion(u8),
    PackageVersion(String),
    SchemaPath(String),
    Dependency(Dependency),
    CodeGeneration(LanguageDetails),
    PublishRegistry((String, PublishRegistry)),
    Settings(crate::settings::SettingsDict),
    /// `handshake = "..."`: the handshake a protocol without its own
    /// `@handshake` runs, canonical. Absent means `comline@1`.
    Handshake(String),
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Dependency {
    pub author: String,
    pub project: String,
    pub version: String
}

/// One entry from `code_generation.languages`, a declared
/// `language#lang_version` the package supports being generated as. It is a
/// *capability declaration* only: which of these a consumer realises, where the
/// code lands, and which historical package versions to emit are all consumer
/// concerns, configured outside the congregation.
#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LanguageDetails {
    pub name: String,
}

// TODO: Optimization might be possible here, and for other structures
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

/// The package-level `settings` dictionary. Always present once config is
/// frozen - `interpret_context` synthesises an empty one when the user
/// writes no `settings` key at all - so `None` here only ever means
/// `units` hasn't been through that freezing pass yet.
pub fn settings(units: &[FrozenUnit]) -> Option<&crate::settings::SettingsDict> {
    units.iter().find_map(|unit| match unit {
        FrozenUnit::Settings(dict) => Some(dict),
        _ => None
    })
}

/// The package's default handshake: its `handshake = "..."`, else
/// [`DEFAULT_HANDSHAKE`](crate::schema::idl::annotations::DEFAULT_HANDSHAKE).
pub fn handshake(units: &[FrozenUnit]) -> &str {
    units
        .iter()
        .find_map(|unit| match unit {
            FrozenUnit::Handshake(name) => Some(name.as_str()),
            _ => None,
        })
        .unwrap_or(crate::schema::idl::annotations::DEFAULT_HANDSHAKE)
}
