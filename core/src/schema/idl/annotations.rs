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

/// Which declaration an `@key=value` annotation attaches to. The grammar
/// permits annotations on a `struct`, a `Field`, a `protocol`, and a
/// `Function` (`grammar.rs`); `Leading` covers the first two — a struct's
/// and a protocol's own annotation sit in the same "top level, right
/// before the keyword" position, indistinguishable without looking past
/// the cursor at text that doesn't exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationScope {
    /// Top level, before `struct` or `protocol`.
    Leading,
    /// Inside a `struct`/`error` body, before a field.
    Field,
    /// Inside a `protocol` body, before a function.
    Function,
}

/// Who actually reads an annotation. Distinct from `consumed_by` (the
/// *names* of consumers) — this is the coarser question of whether core
/// itself has an opinion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationAuthority {
    /// Core itself reads/validates it (`@validators`, via
    /// `ir::validation::validator`).
    Core,
    /// A target-language generator reads it; core freezes it as an opaque
    /// `Property` and has no opinion on its meaning (`@framing`,
    /// `@timeout_ms`).
    Target,
    /// Decided and documented (see `design/core-target-contract.md`); no
    /// consumer reads it yet.
    Advisory,
}

/// Everything known about one `@key`.
pub struct AnnotationInfo {
    pub key: &'static str,
    pub scope: AnnotationScope,
    pub authority: AnnotationAuthority,
    /// One-line summary of what it does.
    pub description: &'static str,
    /// Plain-text behavior when the annotation is absent.
    pub default: &'static str,
    /// The expected value's shape, for display.
    pub value: &'static str,
    /// Named consumers, as data rather than prose. Empty — not
    /// necessarily only for `authority: Advisory` — means nothing reads
    /// it yet.
    pub consumed_by: &'static [&'static str],
}

pub const KNOWN_ANNOTATIONS: &[AnnotationInfo] = &[
    AnnotationInfo {
        key: "validators",
        scope: AnnotationScope::Field,
        authority: AnnotationAuthority::Core,
        description: "Attaches one or more named validators to this field.",
        default: "no validators run",
        value: "a list of validator calls, e.g. `[StringBounds(min_chars = 3)]`",
        consumed_by: &["`comline build`'s validation pass"],
    },
    AnnotationInfo {
        key: "timeout_ms",
        scope: AnnotationScope::Function,
        authority: AnnotationAuthority::Target,
        description: "How long the client waits for the response before timing out. \
                       Request/response calls only — a one-way call has nothing to wait for.",
        default: "no timeout — waits indefinitely",
        value: "an integer, in milliseconds",
        consumed_by: &["the generated Rust client (`comline-rust`)"],
    },
    AnnotationInfo {
        key: "idempotent",
        scope: AnnotationScope::Function,
        authority: AnnotationAuthority::Advisory,
        description: "Marks that calling this function twice has the same effect as calling \
                       it once — safe to retry.",
        default: "not idempotent",
        value: "a bare marker — no `=value`",
        consumed_by: &[],
    },
    AnnotationInfo {
        key: "framing",
        scope: AnnotationScope::Leading,
        authority: AnnotationAuthority::Target,
        description: "The wire framing the generated client/server use for this protocol. \
                       No effect on a struct.",
        default: "datagram — Comline's compact binary framing",
        value: "one of `\"jsonrpc\"` (`\"json-rpc\"`, `\"jsonrpc-2.0\"`), or `\"datagram\"` \
                to opt a single protocol back out of a package-wide default",
        consumed_by: &["comline-rust's generator", "comline-typescript's generator"],
    },
];

/// Look up a known annotation by its key (the part right after `@`, no
/// `=value`). `None` for anything not in [`KNOWN_ANNOTATIONS`] — including
/// a perfectly valid, real annotation this table just doesn't know about
/// yet (open namespace; see the module doc).
pub fn lookup(key: &str) -> Option<&'static AnnotationInfo> {
    KNOWN_ANNOTATIONS.iter().find(|a| a.key == key)
}

/// Every known annotation valid in `scope`, in table order.
pub fn for_scope(scope: AnnotationScope) -> impl Iterator<Item = &'static AnnotationInfo> {
    KNOWN_ANNOTATIONS.iter().filter(move |a| a.scope == scope)
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
        assert!(lookup("nope").is_none());
        assert_eq!(for_scope(AnnotationScope::Function).count(), 2);
    }
}
