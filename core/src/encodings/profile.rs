//! The profile schema: what an encoding's `profile` file says about how it
//! encodes, as facts the version comparison turns into bumps.
//!
//! A pinned copy of `Profile`, `ProfileKind`, `Fact` and the fact list from
//! ComlineProject/runtime `runtime/conformance/src/lib.rs`, at rev
//! `257bc027d089041913a6f944072347171168472f`. The runtime corpus is where
//! the profiles are proven; core only checks that a declared profile is one.
//! When the runtime's schema moves, this copy moves with it, and the rev
//! above changes.
//!
//! One addition: [`ProfileKind::Handshake`]. The runtime has no handshake
//! profiles yet, so a handshake's profile states no facts.

// Standard Uses
use std::collections::BTreeMap;

// External Uses
use serde_derive::{Deserialize, Serialize};


/// An encoding's profile (`profiles/<name>.json`): facts about how it
/// encodes, which the version comparison turns into bumps. Each fact names
/// the vector cases that prove it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub kind: ProfileKind,
    pub facts: BTreeMap<String, Fact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Format,
    Framing,
    /// Core's addition: not in the runtime's copy yet.
    Handshake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub value: String,
    pub proven_by: Vec<String>,
}

impl ProfileKind {
    /// Every fact a profile of this kind states, with the values it may
    /// take. The set is fixed: a new fact is a change to the comparison too.
    pub fn facts(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            ProfileKind::Format => &[
                ("struct_fields", &["by_position", "by_name"]),
                ("unknown_field", &["rejected", "ignored"]),
                ("missing_optional", &["rejected", "absent"]),
                ("enum_variants", &["by_name", "by_index"]),
                ("unknown_variant", &["rejected", "tolerated"]),
                ("integer_width", &["by_value", "exact"]),
            ],
            ProfileKind::Framing => &[
                ("calls", &["by_index", "by_name", "by_id"]),
                ("errors", &["by_ordinal", "by_name"]),
            ],
            ProfileKind::Handshake => &[],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProfileKind::Format => "format",
            ProfileKind::Framing => "framing",
            ProfileKind::Handshake => "handshake",
        }
    }
}

impl Profile {
    /// Parse a profile and check it states exactly its kind's facts, each
    /// with a value that fact may take. The error is a sentence.
    pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
        let profile: Profile =
            serde_json::from_slice(bytes).map_err(|e| format!("it isn't a profile: {e}"))?;
        let known = profile.kind.facts();
        for (fact, stated) in &profile.facts {
            let Some((_, values)) = known.iter().find(|(name, _)| name == fact) else {
                return Err(format!(
                    "it states `{fact}`, which isn't a fact of a {} profile",
                    profile.kind.as_str()
                ));
            };
            if !values.contains(&stated.value.as_str()) {
                return Err(format!(
                    "its `{fact}` is `{}`: expected one of {}",
                    stated.value,
                    values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        if let Some((missing, _)) = known.iter().find(|(name, _)| !profile.facts.contains_key(*name)) {
            return Err(format!("it doesn't state `{missing}`"));
        }
        Ok(profile)
    }
}
