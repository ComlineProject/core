// Relative Modules
pub mod cas;
#[cfg(feature = "compiler")]
pub mod loader;

// Standard Uses
use std::slice::Iter;
use std::iter::FilterMap;

// Crate Uses
#[cfg(feature = "compiler")]
use crate::package::config::ir::context::ProjectContext;

// External Uses
use serde_derive::{Serialize, Deserialize};


#[cfg(feature = "compiler")]
pub type FrozenWhole = (ProjectContext, Vec<FrozenUnit>);


/// Persisted into `.comline/` as CBOR, same as `schema::ir::frozen::unit::FrozenUnit`
/// (a separate enum of the same name, one level up in this module tree) -
/// see that type's own doc comment for what that means for safely
/// growing/reordering this one.
#[derive(Debug, Eq, PartialEq, Clone)]
#[derive(Deserialize, Serialize)]
pub enum FrozenUnit {
    Namespace(String),
    /// `id = "..."`: the package's identity, a random id tooling writes
    /// (`comline new`, or `comline build` for a package without one). A
    /// rename keeps it; a copy must change it. Checked by
    /// [`crate::package::id::check`].
    PackageId(String),
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
    /// `wire = { format = "...", framing = "..." }`: what the package speaks
    /// when a protocol doesn't say, each canonical. An entry left out is
    /// `None`, and means the built-in default.
    Wire {
        format: Option<String>,
        framing: Option<String>,
    },
    /// One entry of `encodings = { "<name>@<n>" = { ... } }`: an encoding
    /// this package declares. `profile_hash` is the blake3 of the profile
    /// file's bytes, hex, as the CAS hashes; `map` the suggested mappings,
    /// by language id.
    ///
    /// The two facts the guarantee check (`schema::ir::unverified`) needs
    /// are frozen beside the hash, so a consumer reads them from the
    /// declaration without the profile file:
    /// - `establishes`: what a handshake's profile states as established
    ///   (`schema_agreement`, `encoding_agreement`); empty for a format or
    ///   framing.
    /// - `has_vectors`: whether the package ships conformance vectors beside
    ///   the profile (`profiles/x.vectors.json` for `profiles/x.json`).
    ///   Always true for an official encoding.
    ///
    /// Both default to the strict reading, nothing established and no
    /// vectors, so a declaration frozen before they existed claims nothing
    /// it never proved.
    Encoding {
        name: String,
        kind: crate::encodings::EncodingKind,
        profile_hash: String,
        map: crate::encodings::EncodingMap,
        #[serde(default)]
        establishes: Vec<crate::schema::ir::frozen::unit::Guarantee>,
        #[serde(default)]
        has_vectors: bool,
    },
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

/// The package's declared `wire` entries, `(format, framing)`, as written
/// (canonical). `None` for one it leaves out.
/// The package's `id`, if its config declares one.
pub fn package_id(units: &[FrozenUnit]) -> Option<&str> {
    units.iter().find_map(|unit| match unit {
        FrozenUnit::PackageId(id) => Some(id.as_str()),
        _ => None,
    })
}

pub fn wire(units: &[FrozenUnit]) -> (Option<&str>, Option<&str>) {
    units
        .iter()
        .find_map(|unit| match unit {
            FrozenUnit::Wire { format, framing } => Some((format.as_deref(), framing.as_deref())),
            _ => None,
        })
        .unwrap_or((None, None))
}

/// The package's wire format: its `wire.format`, else
/// [`DEFAULT_FORMAT`](crate::schema::idl::annotations::DEFAULT_FORMAT).
pub fn wire_format(units: &[FrozenUnit]) -> &str {
    wire(units).0.unwrap_or(crate::schema::idl::annotations::DEFAULT_FORMAT)
}

/// The package's default framing: its `wire.framing`, else
/// [`DEFAULT_FRAMING`](crate::schema::idl::annotations::DEFAULT_FRAMING).
pub fn wire_framing(units: &[FrozenUnit]) -> &str {
    wire(units).1.unwrap_or(crate::schema::idl::annotations::DEFAULT_FRAMING)
}
