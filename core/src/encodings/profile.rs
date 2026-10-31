//! The profile schema: what an encoding's `profile` file says about how it
//! encodes, as facts the version comparison turns into bumps.
//!
//! A pinned copy of `Profile`, `ProfileKind`, `Fact` and the fact list from
//! ComlineProject/runtime `runtime/conformance/src/lib.rs`, at rev
//! `64505b40b39c93c5db40e981ba3eef14ecc0fc90` (spec 0.2). The runtime corpus
//! is where the profiles are proven; core only checks that a declared
//! profile is one. When the runtime's schema moves, this copy moves with it,
//! and the rev above changes.
//!
//! A handshake's facts, `schema_agreement` and `encoding_agreement`, are
//! what it establishes; one it doesn't state as `true` is given up
//! ([`Profile::establishes`]).

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::schema::ir::frozen::unit::Guarantee;

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
    /// A handshake: what it establishes between the two ends. A fact it
    /// leaves out, or states `false`, isn't established.
    Handshake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    /// The fact's value. A handshake's facts are JSON booleans, read as
    /// `"true"` / `"false"`; every other fact is a string.
    #[serde(with = "fact_value")]
    pub value: String,
    /// The cases that prove it. A handshake fact stated `false` claims
    /// nothing, so it names none.
    #[serde(default)]
    pub proven_by: Vec<String>,
}

/// A fact's value: a string, or a JSON boolean (a handshake's facts), held
/// as `"true"` / `"false"` and written back as a boolean.
mod fact_value {
    use serde::{Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Written {
        Bool(bool),
        Str(String),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(match Written::deserialize(d)? {
            Written::Bool(b) => b.to_string(),
            Written::Str(s) => s,
        })
    }

    pub fn serialize<S: Serializer>(value: &str, s: S) -> Result<S::Ok, S::Error> {
        match value {
            "true" => s.serialize_bool(true),
            "false" => s.serialize_bool(false),
            other => s.serialize_str(other),
        }
    }
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
            ProfileKind::Handshake => &[
                ("schema_agreement", &["true", "false"]),
                ("encoding_agreement", &["true", "false"]),
            ],
        }
    }

    /// Whether a profile of this kind must state every fact. A handshake's
    /// may leave one out, which reads as `false`: not established.
    pub fn states_every_fact(self) -> bool {
        !matches!(self, ProfileKind::Handshake)
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
        if profile.kind.states_every_fact() {
            if let Some((missing, _)) = known.iter().find(|(name, _)| !profile.facts.contains_key(*name)) {
                return Err(format!("it doesn't state `{missing}`"));
            }
        }
        Ok(profile)
    }

    /// The guarantees a handshake's profile states as established (its
    /// facts that are `true`), sorted. Empty for a format or framing.
    pub fn establishes(&self) -> Vec<Guarantee> {
        if self.kind != ProfileKind::Handshake {
            return vec![];
        }
        let stated = |fact: &str| self.facts.get(fact).is_some_and(|f| f.value == "true");
        let mut set = vec![];
        if stated("schema_agreement") {
            set.push(Guarantee::SchemaAgreement);
        }
        if stated("encoding_agreement") {
            set.push(Guarantee::EncodingAgreement);
        }
        set
    }
}
