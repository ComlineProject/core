//! The profile schema: what an encoding's `profile` file says about how it
//! encodes, as facts the version comparison turns into bumps.
//!
//! A pinned copy of `Profile`, `ProfileKind`, `Fact` and the fact list from
//! ComlineProject/runtime `runtime/conformance/src/profile.rs`, at rev
//! `cf5e06780793d8e71b0ec1133da8fd6c0e42b8e9` (spec 0.10), which reads the
//! profile format ComlineProject/specification defines (`specs/profile.cue`).
//! The specification's vectors are where the profiles are proven; core only
//! checks that a declared profile is one. When the profile format moves, this
//! copy moves with it, and the rev above changes.
//!
//! A handshake's facts, `schema_agreement` and `encoding_agreement`, are
//! what it establishes; one it doesn't state as `true` is given up
//! ([`Profile::establishes`]).

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::schema::ir::frozen::unit::Guarantee;

// External Uses
use comline_catalog::fact;
use serde_derive::{Deserialize, Serialize};


/// An encoding's profile (`profiles/<name>.json`): facts about how it
/// encodes, which the version comparison turns into bumps. Each fact names
/// the vector cases that prove it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Where the file finds its schema, which a reader ignores.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
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
    /// take, in the order profiles list them: the specification's catalog.
    /// The set is fixed: a new fact is a change to the comparison too.
    pub fn facts(self) -> impl Iterator<Item = &'static comline_catalog::Fact> {
        comline_catalog::facts_of(self.catalog_kind())
    }

    /// The catalog's name for this kind.
    pub fn catalog_kind(self) -> comline_catalog::Kind {
        match self {
            ProfileKind::Format => comline_catalog::Kind::Format,
            ProfileKind::Framing => comline_catalog::Kind::Framing,
            ProfileKind::Handshake => comline_catalog::Kind::Handshake,
        }
    }

    /// Whether a profile of this kind must state every fact. A handshake's
    /// may leave one out, which reads as `false`: not established.
    pub fn states_every_fact(self) -> bool {
        !matches!(self, ProfileKind::Handshake)
    }

    pub fn as_str(self) -> &'static str {
        self.catalog_kind().as_str()
    }
}

impl Profile {
    /// Parse a profile and check it states exactly its kind's facts, each
    /// with a value that fact may take. The error is a sentence.
    pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
        let profile: Profile =
            serde_json::from_slice(bytes).map_err(|e| format!("it isn't a profile: {e}"))?;
        for (fact, stated) in &profile.facts {
            let Some(known) = profile.kind.facts().find(|known| known.id == fact) else {
                return Err(format!(
                    "it states `{fact}`, which isn't a fact of a {} profile",
                    profile.kind.as_str()
                ));
            };
            if !known.allows(&stated.value) {
                return Err(format!(
                    "its `{fact}` is `{}`: expected one of {}",
                    stated.value,
                    known.values.iter().map(|v| format!("`{}`", v.id)).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        if profile.kind.states_every_fact() {
            if let Some(missing) = profile.kind.facts().find(|known| !profile.facts.contains_key(known.id)) {
                return Err(format!("it doesn't state `{}`", missing.id));
            }
        }
        Ok(profile)
    }

    /// The grammar a framing writes a payload into (`payload`) or a format
    /// writes in (`output`), if any: `None` for a framing that carries a
    /// payload as bytes, and for a format that writes bytes.
    pub fn grammar(&self) -> Option<String> {
        let (fact, none) = match self.kind {
            ProfileKind::Format => (fact::OUTPUT, "bytes"),
            ProfileKind::Framing => (fact::PAYLOAD, "opaque"),
            ProfileKind::Handshake => return None,
        };
        self.facts.get(fact).map(|f| f.value.clone()).filter(|value| value != none)
    }

    /// What a framing states as `fact`; `None` for one this kind doesn't state.
    pub fn stated(&self, fact: &str) -> Option<String> {
        self.facts.get(fact).map(|f| f.value.clone())
    }

    /// The guarantees a handshake's profile states as established (its
    /// facts that are `true`, each establishing the guarantee the catalog
    /// names), sorted. Empty for a format or framing.
    pub fn establishes(&self) -> Vec<Guarantee> {
        if self.kind != ProfileKind::Handshake {
            return vec![];
        }
        let mut set: Vec<Guarantee> = self
            .kind
            .facts()
            .filter(|known| self.facts.get(known.id).is_some_and(|f| f.value == "true"))
            // One core doesn't know isn't established: the protocol gives it
            // up, which fails closed. A test keeps core and the catalog
            // agreeing on every guarantee.
            .filter_map(|known| known.establishes.and_then(Guarantee::from_name))
            .collect();
        set.sort();
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value core's logic compares a fact with (`diff`, `compat`,
    /// `unverified`, [`Profile::grammar`], [`Profile::establishes`]) is one
    /// the catalog lets that fact take, so none can silently never match.
    #[test]
    fn the_values_core_acts_on_are_the_catalogs() {
        let relied_on = [
            (fact::STRUCT_FIELDS, "by_name"),
            (fact::UNKNOWN_FIELD, "ignored"),
            (fact::MISSING_OPTIONAL, "absent"),
            (fact::ENUM_VARIANTS, "by_name"),
            (fact::OUTPUT, "bytes"),
            (fact::OUTPUT, "json"),
            (fact::CALLS, "by_name"),
            (fact::CALLS, "by_id"),
            (fact::DIRECTIONS, "one"),
            (fact::ERRORS, "by_name"),
            (fact::PAYLOAD, "opaque"),
            (fact::PAYLOAD, "json"),
            (fact::SCHEMA_AGREEMENT, "true"),
            (fact::ENCODING_AGREEMENT, "true"),
        ];
        for (name, value) in relied_on {
            let known = comline_catalog::fact(name).unwrap();
            assert!(known.allows(value), "`{name}` can't be `{value}`");
        }
    }

    #[test]
    fn a_profile_is_held_to_the_catalogs_facts() {
        let profile = |facts: &str| format!(r#"{{ "name": "x@1", "kind": "framing", "facts": {{ {facts} }} }}"#);
        let fact = |name: &str, value: &str| format!(r#""{name}": {{ "value": "{value}", "proven_by": ["x/a"] }}"#);
        let all = [fact("calls", "by_name"), fact("directions", "both"), fact("errors", "by_name"), fact("payload", "opaque")];
        assert!(Profile::parse(profile(&all.join(", ")).as_bytes()).is_ok());

        let wrong = [fact("calls", "by_guess"), all[1].clone(), all[2].clone(), all[3].clone()];
        let error = Profile::parse(profile(&wrong.join(", ")).as_bytes()).unwrap_err();
        assert_eq!(error, "its `calls` is `by_guess`: expected one of `by_index`, `by_name`, `by_id`");

        let error = Profile::parse(profile(&all[1..].join(", ")).as_bytes()).unwrap_err();
        assert_eq!(error, "it doesn't state `calls`");

        let extra = [all.join(", "), fact("struct_fields", "by_name")];
        let error = Profile::parse(profile(&extra.join(", ")).as_bytes()).unwrap_err();
        assert_eq!(error, "it states `struct_fields`, which isn't a fact of a framing profile");
    }
}
