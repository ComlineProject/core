//! Encodings a package declares (`encodings = { ... }` in `config.idp`):
//! formats, framings and handshakes beyond the official ones, each with a
//! profile and suggested mappings per language (core#120).
//!
//! The official names (`packed@1`, `datagram@2`, ...) are declared the same
//! way, by the embedded official package ([`official`]), which every package
//! sees as it sees std. Any other name has to be declared by the package or
//! one of its dependencies.

// Relative Modules
pub mod official;
pub mod profile;

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::FrozenUnit;

// External Uses
use serde_derive::{Deserialize, Serialize};

pub use profile::{Profile, ProfileKind};


/// What an encoding is. The same set as [`ProfileKind`], whose profile it
/// must have.
pub type EncodingKind = ProfileKind;

/// A declaring package's suggested mapping of an encoding onto one
/// language: the type implementing it, and optionally the package and
/// version that type comes from.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncodingMapping {
    #[serde(rename = "type")]
    pub ty: String,
    pub package: Option<String>,
    pub version: Option<String>,
}

/// Which encoding names a package may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePolicy {
    /// Namespaced names only (`com.acme.x@1`). Every package but the
    /// official one.
    User,
    /// Bare names too (`json@1`): the official encodings package.
    Official,
}

/// **The hook:** which names `context` may declare. The embedded official
/// package ([`official`]) may declare bare names; every other package is a
/// [`NamePolicy::User`] one. Decided by where the package came from, not by
/// its name, so a user package called `comline` is still a user package.
pub(crate) fn name_policy(context: &ProjectContext) -> NamePolicy {
    use crate::package::config::ir::context::Origin;
    match context.origin {
        Origin::Official => NamePolicy::Official,
        Origin::Virtual | Origin::Disk(_) => NamePolicy::User,
    }
}

/// Whether `name` is a namespaced encoding name: reverse-DNS-like
/// segments, at least one dot before the base name, then `@<n>`
/// (`com.acme.hello@1`).
pub fn is_namespaced(name: &str) -> bool {
    match split_version(name) {
        Some((base, _)) => {
            let segments: Vec<&str> = base.split('.').collect();
            segments.len() >= 2 && segments.iter().all(|s| is_segment(s))
        }
        None => false,
    }
}

/// Whether `name` is a bare encoding name: one segment, then `@<n>`
/// (`json@1`). Reserved to the official encodings.
pub fn is_bare(name: &str) -> bool {
    matches!(split_version(name), Some((base, _)) if is_segment(base))
}

fn split_version(name: &str) -> Option<(&str, &str)> {
    let (base, version) = name.rsplit_once('@')?;
    (!version.is_empty() && version.chars().all(|c| c.is_ascii_digit())).then_some((base, version))
}

fn is_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Check a name a package declares under `encodings`. The error is a
/// sentence.
pub fn check_declared_name(name: &str, policy: NamePolicy) -> Result<(), String> {
    if split_version(name).is_none() {
        return Err(format!(
            "encoding `{name}` needs a version: write it as `<name>@<n>`, such as `{}@1`",
            name.trim_end_matches('@')
        ));
    }
    if is_namespaced(name) {
        return Ok(());
    }
    if is_bare(name) {
        return match policy {
            NamePolicy::Official => Ok(()),
            NamePolicy::User => Err(format!(
                "encoding `{name}`: bare names are reserved to official encodings; use a \
                 namespaced name such as com.acme.{name}"
            )),
        };
    }
    Err(format!(
        "encoding `{name}` isn't a valid name: write a namespaced name such as \
         com.acme.x@1 (lowercase segments separated by dots, then `@<n>`)"
    ))
}

/// The encodings a package's frozen config declares, by name.
pub fn declared(units: &[FrozenUnit]) -> impl Iterator<Item = (&str, EncodingKind)> {
    units.iter().filter_map(|unit| match unit {
        FrozenUnit::Encoding { name, kind, .. } => Some((name.as_str(), *kind)),
        _ => None,
    })
}

/// Whether `name` is declared as a `kind` by the official package, the
/// package `context` holds, or any of its dependencies. An official name may
/// be written as any of its spellings (`json-rpc`).
pub fn is_declared(context: &ProjectContext, kind: EncodingKind, name: &str) -> bool {
    if official::resolve(kind, name).is_some() {
        return true;
    }
    let own = context.config_frozen.iter().flatten();
    let dependencies = context.dependency_configs.iter().flat_map(|(_, units)| units);
    declared_in(own.chain(dependencies), kind, name)
}

fn declared_in<'a>(
    units: impl Iterator<Item = &'a FrozenUnit>,
    kind: EncodingKind,
    name: &str,
) -> bool {
    units.into_iter().any(|unit| {
        matches!(unit, FrozenUnit::Encoding { name: n, kind: k, .. } if n == name && *k == kind)
    })
}

/// The error for a namespaced `name` used as a `kind` that nothing
/// declares.
pub fn undeclared(kind: EncodingKind, name: &str) -> String {
    format!(
        "{} `{name}` isn't declared: declare it under `encodings` in this package's \
         config.idp, or depend on a package that does",
        kind.as_str()
    )
}

/// `map = { <language> = { type = "...", package = "...", version = "..." } }`,
/// already read into entries; sorted by language.
pub type EncodingMap = BTreeMap<String, EncodingMapping>;

use crate::diagnostics::DiagnosticCode;

/// An encoding's name is malformed, reserved, or declared twice.
pub const ENCODING_NAME: DiagnosticCode = DiagnosticCode::error("config::encoding-name");
/// An encoding's entry is malformed (its `kind` or `map`).
pub const ENCODING_ENTRY: DiagnosticCode = DiagnosticCode::error("config::encoding");
/// An encoding's profile is missing, malformed, or of another kind.
pub const ENCODING_PROFILE: DiagnosticCode = DiagnosticCode::error("config::encoding-profile");
