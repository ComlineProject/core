//! Known `@key=value` annotations — metadata for consumers that need to
//! explain one, not an enforcement list. Same spirit as [`super::size`]
//! and [`super::vocabulary`]: core hosts this because it's a fact about
//! the language that would otherwise exist nowhere as structured data
//! (only as prose in `docs/docs/docs/design/core-target-contract.md`'s
//! "Per-call settings" section), not because core itself acts on every
//! entry.
//!
//! `@key=value` is an open namespace: any key parses and freezes
//! regardless of whether it's listed here ([`FrozenUnit::Property`]'s
//! `expression` happily carries an unrecognised one). This table is
//! deliberately a curated, known-good subset — the keys a real generator
//! or core's own validation pass actually reads today — not an exhaustive
//! or enforced list. An unrecognised key is not an error.
//!
//! [`FrozenUnit::Property`]: crate::schema::ir::frozen::unit::FrozenUnit::Property
//!
//! This is a static table, not a distributed registry `comline-rust`/
//! `comline-typescript` contribute their own entries into. Deliberate:
//! the language server and the playground editor need to show `@framing`
//! help without linking any generator crate, and both a cross-crate
//! dependency and a `linkme`/`inventory`-style compile-time registry would
//! invert that — the latter also fights `wasm-opt`'s dead-code stripping
//! under the playground's release profile. [`AnnotationAuthority`] and
//! `consumed_by: &[&str]` already describe the distributed world on paper;
//! a future registry would only need to change how [`lookup`]/[`for_scope`]
//! are implemented, not [`AnnotationInfo`]'s shape.

use crate::config_value::{StrVariant, ValueType};

/// Which declaration an `@key=value` annotation attaches to. The grammar
/// permits annotations on a `struct`, an `error`, a `Field`, a `protocol`,
/// a `Function` and a `type` alias (`grammar.rs`); `Leading` covers the
/// first three — a
/// struct's, an error's, and a protocol's own annotation sit in the same
/// "top level, right before the keyword" position, indistinguishable
/// without looking past the cursor at text that doesn't exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationScope {
    /// Top level, before `struct`, `error`, or `protocol`.
    Leading,
    /// Top level, before `type`. The same position as [`Self::Leading`];
    /// an editor that can see the keyword after it can tell them apart.
    TypeAlias,
    /// Inside a `struct`/`error` body, before a field.
    Field,
    /// Inside a `protocol` body, before a function.
    Function,
    /// Valid everywhere the grammar permits an annotation at all - matches
    /// every [`for_scope`] query regardless of which specific scope is
    /// asked for (`@settings` is the only entry using this today).
    Universal,
}

/// Who actually reads an annotation. Distinct from `consumed_by` (the
/// *names* of consumers) — this is the coarser question of whether core
/// itself has an opinion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationAuthority {
    /// Core itself reads/validates it (`@validators`, via
    /// `ir::validation::validator`).
    Core,
    /// A target-language generator decides what it *means*; core checks
    /// only that the value has the declared shape (`@framing`,
    /// `@timeout_ms`). Core used to have no opinion at all here and froze
    /// the value as opaque text - so `@timeout_ms = "abc"` and
    /// `@framing = "nonsense"` both compiled clean and were silently
    /// dropped by whichever generator tried to read them.
    Target,
    /// Decided and documented (see `design/core-target-contract.md`); no
    /// consumer reads it yet.
    Advisory,
}

/// The canonical spellings of `@framing`'s wire framings, with every
/// accepted alias. The single place this set lives: before this it existed
/// as prose here plus four independent copies in the consumers, one of
/// which (the playground) was missing `json-rpc` and so rendered a
/// JSON-RPC protocol as datagram.
const FRAMINGS: &[StrVariant] = &[
    StrVariant::with_aliases("jsonrpc", &["json-rpc", "jsonrpc-2.0"]),
    StrVariant::with_aliases("datagram", &["comline.datagram"]),
];

/// The handshakes `@handshake` and the package's `handshake = "..."` accept.
/// Versioned like encodings; `none` has nothing to version. A namespaced
/// custom one (`com.acme.hello@1`) arrives with third-party encodings
/// (core#120) - see [`check_handshake`].
const HANDSHAKES: &[StrVariant] = &[
    StrVariant::new(DEFAULT_HANDSHAKE),
    StrVariant::new(HANDSHAKE_NONE),
];

/// The handshake a protocol runs when neither it nor its package names one.
pub const DEFAULT_HANDSHAKE: &str = "comline@1";
/// The official "no handshake": nothing sent, nothing checked.
pub const HANDSHAKE_NONE: &str = "none";

/// Check a written handshake name, returning its canonical spelling. The
/// error is a full sentence: a namespaced custom name gets its own, since
/// it's well-formed and only waiting on core#120, not a typo.
pub fn check_handshake(written: &str) -> Result<&'static str, String> {
    if let Some(known) = canonical_value("handshake", written) {
        return Ok(known);
    }
    let known = format!("`{DEFAULT_HANDSHAKE}` or `{HANDSHAKE_NONE}`");
    match written.split_once('@') {
        Some((name, version))
            if name.contains('.') && !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) =>
        {
            Err(format!(
                "handshake `{written}` isn't known: only {known} exist yet; custom handshakes \
                 arrive with third-party encodings (core#120)"
            ))
        }
        _ => Err(format!("handshake `{written}` isn't known: expected {known}")),
    }
}

/// Everything known about one `@key`.
pub struct AnnotationInfo {
    pub key: &'static str,
    /// Every scope it's valid in.
    pub scopes: &'static [AnnotationScope],
    pub authority: AnnotationAuthority,
    /// One-line summary of what it does.
    pub description: &'static str,
    /// Plain-text behavior when the annotation is absent.
    pub default: &'static str,
    /// What this annotation's value must be. Machine-readable, so core
    /// can validate it, a generator can canonicalise it, and the editor
    /// can offer its real values - previously a prose string that only a
    /// human could read, which is why each consumer kept its own copy of
    /// e.g. `@framing`'s accepted set.
    pub value: ValueType,
    /// Named consumers, as data rather than prose. Empty — not
    /// necessarily only for `authority: Advisory` — means nothing reads
    /// it yet.
    pub consumed_by: &'static [&'static str],
}

pub const KNOWN_ANNOTATIONS: &[AnnotationInfo] = &[
    AnnotationInfo {
        key: "validators",
        scopes: &[AnnotationScope::Field, AnnotationScope::TypeAlias],
        authority: AnnotationAuthority::Core,
        description: "Attaches one or more named validators to this field, or to this \
                       type alias (checked wherever the alias is used).",
        default: "no validators run",
        value: ValueType::ValidatorList,
        consumed_by: &["`comline build`'s validation pass"],
    },
    AnnotationInfo {
        key: "timeout_ms",
        scopes: &[AnnotationScope::Function],
        authority: AnnotationAuthority::Target,
        description: "How long, in milliseconds, the client waits for the response before \
                       timing out. Request/response calls only — a one-way call has nothing \
                       to wait for.",
        default: "no timeout — waits indefinitely",
        value: ValueType::Integer,
        consumed_by: &["the generated Rust client (`comline-rust`)"],
    },
    AnnotationInfo {
        key: "idempotent",
        scopes: &[AnnotationScope::Function],
        authority: AnnotationAuthority::Advisory,
        description: "Marks that calling this function twice has the same effect as calling \
                       it once — safe to retry.",
        default: "not idempotent",
        value: ValueType::Marker,
        consumed_by: &[],
    },
    AnnotationInfo {
        key: "framing",
        scopes: &[AnnotationScope::Leading],
        authority: AnnotationAuthority::Target,
        description: "The wire framing the generated client/server use for this protocol. \
                       No effect on a struct.",
        default: "datagram — Comline's compact binary framing",
        value: ValueType::StrEnum(FRAMINGS),
        consumed_by: &["comline-rust's generator", "comline-typescript's generator"],
    },
    AnnotationInfo {
        key: "handshake",
        scopes: &[AnnotationScope::Leading],
        authority: AnnotationAuthority::Target,
        description: "What runs on a connection before the first call: `comline@1` checks that \
                       both ends share the schema, format and framing; `none` sends and checks \
                       nothing. Overrides the package's `handshake` in `config.idp`. No effect \
                       on a struct.",
        default: "the package's `handshake`, else comline@1",
        value: ValueType::StrEnum(HANDSHAKES),
        consumed_by: &["comline-rust's generator", "comline-typescript's generator"],
    },
    AnnotationInfo {
        key: "settings",
        scopes: &[AnnotationScope::Universal],
        authority: AnnotationAuthority::Core,
        description: "Opts a declaration into a named `settings NAME { ... }` preset declared \
                       in this schema, merged onto whatever settings already apply to it.",
        default: "no preset applied — only the package/schema-level settings apply",
        value: ValueType::Name,
        consumed_by: &[],
    },
    AnnotationInfo {
        key: "deprecated",
        scopes: &[AnnotationScope::Leading, AnnotationScope::TypeAlias],
        authority: AnnotationAuthority::Core,
        description: "Marks a struct or a type alias as deprecated. Referencing it from a field, a function \
                       argument/return, or a type alias target is a warning, naming this \
                       declaration and (if given) the message. Parses on an error, a protocol, \
                       a field, or a function too, but nothing warns on those yet - none of \
                       them have a usage site this check can reach.",
        default: "not deprecated",
        value: ValueType::BoolOrStr,
        consumed_by: &["core's own validation pass (`ir::validation::validator`)"],
    },
];

/// The human sentence describing what `@key`'s value must be - generated
/// from [`AnnotationInfo::value`] rather than written out, so the prose and
/// the check can never disagree.
pub fn describe_value(info: &AnnotationInfo) -> String {
    crate::config_value::describe(&info.value)
}

/// Fold a written value onto the canonical spelling its annotation
/// declares, e.g. `@framing = "json-rpc"` to `jsonrpc`. `None` for a key
/// with no closed value set, or a value that isn't an accepted spelling.
///
/// **Every consumer that acts on a closed-set annotation should call
/// this** rather than matching the spellings itself. Each generator used
/// to keep its own copy of `@framing`'s aliases, and they had already
/// drifted: the playground's omitted `json-rpc`, so a protocol the real
/// generators emitted as JSON-RPC rendered as datagram there.
pub fn canonical_value(key: &str, written: &str) -> Option<&'static str> {
    let info = lookup(key)?;
    crate::config_value::canonicalize(&info.value, written)
}

/// Every spelling `@key` offers for completion - canonical forms only.
/// Empty for a key with no closed set.
pub fn value_spellings(key: &str) -> Vec<&'static str> {
    lookup(key).map(|i| crate::config_value::spellings(&i.value)).unwrap_or_default()
}

/// Look up a known annotation by its key (the part right after `@`, no
/// `=value`). `None` for anything not in [`KNOWN_ANNOTATIONS`] — including
/// a perfectly valid, real annotation this table just doesn't know about
/// yet (open namespace; see the module doc).
pub fn lookup(key: &str) -> Option<&'static AnnotationInfo> {
    KNOWN_ANNOTATIONS.iter().find(|a| a.key == key)
}

/// Every known annotation valid in `scope`, in table order. A
/// [`AnnotationScope::Universal`] entry (e.g. `@settings`) matches every
/// scope, not just a literal equal one.
pub fn for_scope(scope: AnnotationScope) -> impl Iterator<Item = &'static AnnotationInfo> {
    KNOWN_ANNOTATIONS
        .iter()
        .filter(move |a| a.scopes.iter().any(|&s| s == scope || s == AnnotationScope::Universal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for a in KNOWN_ANNOTATIONS {
            assert!(seen.insert(a.key), "duplicate annotation key: {}", a.key);
        }
    }

    #[test]
    fn lookup_and_for_scope_agree_with_the_table() {
        assert!(lookup("validators").is_some());
        assert!(lookup("deprecated").is_some());
        assert!(lookup("nope").is_none());
        // timeout_ms, idempotent (both Function), plus settings (Universal).
        assert_eq!(for_scope(AnnotationScope::Function).count(), 3);
        // framing, handshake, deprecated (all Leading), plus settings (Universal).
        assert_eq!(for_scope(AnnotationScope::Leading).count(), 4);
        // validators, deprecated, plus settings (Universal).
        let mut alias: Vec<_> = for_scope(AnnotationScope::TypeAlias).map(|a| a.key).collect();
        alias.sort();
        assert_eq!(alias, ["deprecated", "settings", "validators"]);
    }

    #[test]
    fn framing_accepts_every_spelling_its_consumers_relied_on() {
        // The union of what the two generators and the playground each
        // matched on their own, before this became one table. The
        // playground's copy was missing `json-rpc` entirely.
        for spelling in ["jsonrpc", "json-rpc", "jsonrpc-2.0"] {
            assert_eq!(canonical_value("framing", spelling), Some("jsonrpc"), "{spelling}");
        }
        for spelling in ["datagram", "comline.datagram"] {
            assert_eq!(canonical_value("framing", spelling), Some("datagram"), "{spelling}");
        }
        assert_eq!(canonical_value("framing", "nonsense"), None);
    }

    #[test]
    fn completion_offers_only_the_canonical_framings() {
        assert_eq!(value_spellings("framing"), vec!["jsonrpc", "datagram"]);
        // A key with no closed set has nothing to offer, rather than
        // the editor inventing something.
        assert!(value_spellings("timeout_ms").is_empty());
        assert!(value_spellings("nope").is_empty());
    }

    #[test]
    fn canonical_value_is_none_for_a_key_without_a_closed_set() {
        assert_eq!(canonical_value("timeout_ms", "100"), None);
        assert_eq!(canonical_value("nope", "whatever"), None);
    }

    #[test]
    fn every_entrys_prose_is_generated_from_its_declared_type() {
        for info in KNOWN_ANNOTATIONS {
            let described = describe_value(info);
            assert!(!described.is_empty(), "{}", info.key);
        }
        // Spot-check that the generated prose actually reflects the type.
        let framing = lookup("framing").unwrap();
        assert!(describe_value(framing).contains("jsonrpc"));
        assert_eq!(describe_value(lookup("timeout_ms").unwrap()), "an integer");
    }

    #[test]
    fn handshake_names_are_checked_and_canonical() {
        assert_eq!(check_handshake("comline@1"), Ok("comline@1"));
        assert_eq!(check_handshake("none"), Ok("none"));
        assert_eq!(value_spellings("handshake"), vec!["comline@1", "none"]);

        let custom = check_handshake("com.acme.hello@1").unwrap_err();
        assert!(custom.contains("core#120"), "{custom}");

        for typo in ["comline", "comline@2", "None", "hello@1", "com.acme.hello"] {
            let err = check_handshake(typo).unwrap_err();
            assert!(!err.contains("core#120"), "{typo}: {err}");
            assert!(err.contains("`comline@1` or `none`"), "{typo}: {err}");
        }
    }

    #[test]
    fn universal_scope_matches_every_scope() {
        for scope in [
            AnnotationScope::Leading,
            AnnotationScope::TypeAlias,
            AnnotationScope::Field,
            AnnotationScope::Function,
        ] {
            assert!(for_scope(scope).any(|a| a.key == "settings"));
        }
    }
}
