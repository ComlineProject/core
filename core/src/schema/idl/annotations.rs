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

use crate::config_value::{DictKey, StrVariant, ValueType};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

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
    StrVariant::with_aliases(FRAMING_DATAGRAM, &["datagram@1", "comline.datagram"]),
    StrVariant::new(FRAMING_DATAGRAM2),
];

/// The original datagram framing (`datagram@1`): a request names its call
/// but not its protocol, so a transport carries one protocol, called one
/// way.
pub const FRAMING_DATAGRAM: &str = "datagram";

/// `datagram@2`: the datagram framing whose frames say whether they're a
/// request or a reply, and which protocol they belong to - what calls in
/// both directions need.
pub const FRAMING_DATAGRAM2: &str = "datagram@2";

/// JSON-RPC's canonical name.
pub const FRAMING_JSONRPC: &str = "jsonrpc";

/// The framing a protocol gets when neither it (`@framing`) nor its package
/// (`wire.framing`) names one: a connection, so several protocols share a
/// transport and calls go both ways.
pub const DEFAULT_FRAMING: &str = FRAMING_DATAGRAM2;

/// The wire formats a package's `wire.format` accepts, by the names the
/// runtimes put in the handshake.
const FORMATS: &[StrVariant] = &[
    StrVariant::new(FORMAT_PACKED),
    StrVariant::new(FORMAT_JSON),
    StrVariant::new(FORMAT_MSGPACK),
];

/// `packed@1`: fixed-width fields back to back, varint lengths, no tags.
pub const FORMAT_PACKED: &str = "packed@1";
/// JSON, a struct as an object.
pub const FORMAT_JSON: &str = "json@1";
/// MessagePack, a struct as an array.
pub const FORMAT_MSGPACK: &str = "msgpack@1";

/// The format a package speaks when its `wire.format` doesn't say.
pub const DEFAULT_FORMAT: &str = FORMAT_PACKED;

/// Check a written wire format name, returning its canonical spelling.
pub fn check_format(written: &str) -> Result<&'static str, String> {
    crate::config_value::canonicalize(&ValueType::StrEnum(FORMATS), written).ok_or_else(|| {
        let known: Vec<String> = FORMATS.iter().map(|f| format!("`{}`", f.canonical)).collect();
        format!(
            "wire format `{written}` isn't known: expected one of {}, or a namespaced format \
             this package or a dependency declares under `encodings`",
            known.join(", ")
        )
    })
}

/// Check a written framing name for the package's `wire.framing`,
/// returning its canonical spelling.
pub fn check_framing(written: &str) -> Result<&'static str, String> {
    canonical_value("framing", written).ok_or_else(|| {
        format!(
            "framing `{written}` isn't known: expected one of {}, or a namespaced framing this \
             package or a dependency declares under `encodings`",
            value_spellings("framing").iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ")
        )
    })
}

/// How JSON-RPC names a call on a connection, `@framing`'s `method_names`:
/// the function alone (`send`), or with its protocol (`chat.Chat/send`).
pub const METHOD_NAMES: &[&str] = &["bare", "qualified"];
const METHOD_NAME_STRINGS: &[StrVariant] = &[StrVariant::new("bare"), StrVariant::new("qualified")];

/// `@framing`'s dictionary form. The bare name is shorthand for
/// `{ name = ... }`.
const FRAMING_KEYS: &[DictKey] = &[
    DictKey { key: "name", value: ValueType::StrEnum(FRAMINGS), required: true },
    DictKey {
        key: "method_names",
        value: ValueType::AnyOf(&[ValueType::Keyword(METHOD_NAMES), ValueType::StrEnum(METHOD_NAME_STRINGS)]),
        required: false,
    },
];

/// How JSON-RPC names calls (`@framing`'s `method_names`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MethodNames {
    /// The function's name alone. The default.
    #[default]
    Bare,
    /// `namespace.Protocol/function`.
    Qualified,
}

/// A protocol's framing, read from either form of `@framing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramingValue {
    /// The canonical name (`jsonrpc`, `datagram`, `datagram@2`).
    pub name: &'static str,
    /// Meaningful under JSON-RPC only.
    pub method_names: MethodNames,
}

impl FramingValue {
    /// A framing written by name: `"jsonrpc"`, or a package's
    /// `wire.framing`. `None` for a name that isn't known.
    pub fn named(written: &str) -> Option<FramingValue> {
        Some(FramingValue { name: canonical_value("framing", written)?, method_names: MethodNames::Bare })
    }

    /// `@framing`'s value, `"name"` or `{ name = ..., method_names = ... }`.
    /// **Every consumer reads `@framing` through this**, so the two forms
    /// can't be told apart anywhere they mean the same. `None` for a value
    /// that isn't a valid framing, which validation reports.
    pub fn of(value: &ConfigValue) -> Option<FramingValue> {
        match value {
            ConfigValue::Str(name) => FramingValue::named(name),
            ConfigValue::Dict(_) => {
                let name = match value.entry("name")? {
                    ConfigValue::Str(name) => name,
                    _ => return None,
                };
                let method_names = match value.entry("method_names") {
                    None => MethodNames::Bare,
                    Some(ConfigValue::Str(m) | ConfigValue::Name(m)) => match m.as_str() {
                        "bare" => MethodNames::Bare,
                        "qualified" => MethodNames::Qualified,
                        _ => return None,
                    },
                    Some(_) => return None,
                };
                Some(FramingValue { method_names, ..FramingValue::named(name)? })
            }
            _ => None,
        }
    }

    /// A protocol's own `@framing`, from its parameters.
    pub fn of_protocol(parameters: &[FrozenUnit]) -> Option<FramingValue> {
        parameters.iter().find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == "framing" => FramingValue::of(value),
            _ => None,
        })
    }
}

/// Which side implements a protocol (`@provider`), or calls it
/// (`@consumer`): the side that dialled (`Client`), the side that accepted
/// (`Server`), or both (`Multiple`).
pub const SIDES: &[&str] = &["Server", "Client", "Multiple"];

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

/// Check a written handshake name against the official ones, returning
/// its canonical spelling. A namespaced name (`com.acme.hello@1`) is a
/// declared one, which [`check_handshake_or_namespaced`] lets through.
pub fn check_handshake(written: &str) -> Result<&'static str, String> {
    canonical_value("handshake", written).ok_or_else(|| {
        format!(
            "handshake `{written}` isn't known: expected `{DEFAULT_HANDSHAKE}`, `{HANDSHAKE_NONE}`, \
             or a namespaced handshake this package or a dependency declares under `encodings`"
        )
    })
}

/// An official name's canonical spelling, or a namespaced name as written:
/// whether a namespaced one is declared is checked once the package's
/// dependencies are known (see [`crate::encodings::is_declared`]).
fn official_or_namespaced(
    official: Result<&'static str, String>,
    written: &str,
) -> Result<String, String> {
    match official {
        Ok(name) => Ok(name.to_string()),
        Err(_) if crate::encodings::is_namespaced(written) => Ok(written.to_string()),
        Err(message) => Err(message),
    }
}

/// [`check_format`], letting a namespaced name through.
pub fn check_format_or_namespaced(written: &str) -> Result<String, String> {
    official_or_namespaced(check_format(written), written)
}

/// [`check_framing`], letting a namespaced name through.
pub fn check_framing_or_namespaced(written: &str) -> Result<String, String> {
    official_or_namespaced(check_framing(written), written)
}

/// [`check_handshake`], letting a namespaced name through.
pub fn check_handshake_or_namespaced(written: &str) -> Result<String, String> {
    official_or_namespaced(check_handshake(written), written)
}

/// Whether `written` names an official encoding of `kind` (today's fixed
/// lists, until the official encodings package replaces them).
pub fn is_official(kind: crate::encodings::EncodingKind, written: &str) -> bool {
    use crate::encodings::EncodingKind;
    match kind {
        EncodingKind::Format => check_format(written).is_ok(),
        EncodingKind::Framing => check_framing(written).is_ok(),
        EncodingKind::Handshake => check_handshake(written).is_ok(),
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
        description: "The wire framing the generated client/server use for this protocol: \
                       its name, or `{ name = \"jsonrpc\", method_names = qualified }` to set \
                       how JSON-RPC names calls (`bare`, the default: `send`; `qualified`: \
                       `chat.Chat/send`, which several protocols on one connection may need). \
                       No effect on a struct.",
        default: "the package's `wire.framing`, else `datagram@2` — Comline's compact binary \
                  framing on a connection (`datagram@1`: one protocol per transport)",
        value: ValueType::AnyOf(&[ValueType::StrEnum(FRAMINGS), ValueType::Dict(FRAMING_KEYS)]),
        consumed_by: &["comline-rust's generator", "comline-typescript's generator"],
    },
    AnnotationInfo {
        key: "provider",
        scopes: &[AnnotationScope::Leading],
        authority: AnnotationAuthority::Core,
        description: "Which side implements this protocol: `Server` (the side that accepted the \
                       connection), `Client` (the side that dialled; the server calls it) or \
                       `Multiple` (both, so calls go either way). Calls to a `Client` or \
                       `Multiple` protocol need a framing that tells requests from replies \
                       (`@framing = \"datagram@2\"` or `\"jsonrpc\"`). No effect on a struct.",
        default: "Server",
        value: ValueType::Keyword(SIDES),
        consumed_by: &["core's validation pass", "comline-rust's generator", "comline-typescript's generator"],
    },
    AnnotationInfo {
        key: "consumer",
        scopes: &[AnnotationScope::Leading],
        authority: AnnotationAuthority::Core,
        description: "Which side calls this protocol: the counterpart of `@provider`, which it \
                       follows from. Writing both is allowed, but they have to agree.",
        default: "the counterpart of `@provider`",
        value: ValueType::Keyword(SIDES),
        consumed_by: &["core's validation pass"],
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
        key: "unverified",
        scopes: &[AnnotationScope::Leading],
        authority: AnnotationAuthority::Core,
        description: "Gives up guarantees Comline otherwise gives this protocol's peers, with \
                       the reason why. Core works out which (a `none` handshake gives up schema \
                       and encoding agreement), and every consumer has to accept them. Required \
                       where something is given up; off by default \
                       (`annotations.unverified.allowed`).",
        default: "nothing given up",
        value: ValueType::Str,
        consumed_by: &["core's validation pass", "the consumer's `accept_unverified`"],
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
        // framing, provider, consumer, handshake, unverified, deprecated (all
        // Leading), plus settings (Universal).
        assert_eq!(for_scope(AnnotationScope::Leading).count(), 7);
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
        for spelling in ["datagram", "datagram@1", "comline.datagram"] {
            assert_eq!(canonical_value("framing", spelling), Some("datagram"), "{spelling}");
        }
        assert_eq!(canonical_value("framing", "nonsense"), None);
    }

    #[test]
    fn a_framing_reads_the_same_in_either_form() {
        let dict = |entries: &[(&str, ConfigValue)]| {
            ConfigValue::Dict(entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
        };
        let str = |s: &str| ConfigValue::Str(s.to_string());
        let jsonrpc = FramingValue { name: "jsonrpc", method_names: MethodNames::Bare };
        assert_eq!(FramingValue::of(&str("json-rpc")), Some(jsonrpc));
        assert_eq!(FramingValue::of(&dict(&[("name", str("jsonrpc"))])), Some(jsonrpc));
        let qualified = FramingValue { method_names: MethodNames::Qualified, ..jsonrpc };
        for written in [ConfigValue::Name("qualified".into()), str("qualified")] {
            assert_eq!(
                FramingValue::of(&dict(&[("name", str("jsonrpc")), ("method_names", written)])),
                Some(qualified)
            );
        }
        assert_eq!(FramingValue::of(&dict(&[("method_names", str("bare"))])), None, "no name");
        assert_eq!(FramingValue::of(&dict(&[("name", str("jsonrpc")), ("method_names", str("loud"))])), None);
        assert_eq!(FramingValue::of(&ConfigValue::Name("jsonrpc".into())), None, "unquoted");
    }

    #[test]
    fn wire_names_are_checked_and_canonical() {
        assert_eq!(check_format("packed@1"), Ok("packed@1"));
        assert_eq!(check_format("msgpack@1"), Ok("msgpack@1"));
        let err = check_format("packed").unwrap_err();
        assert!(err.contains("`packed@1`, `json@1`, `msgpack@1`"), "{err}");
        assert_eq!(check_framing("datagram@1"), Ok("datagram"));
        assert_eq!(check_framing("json-rpc"), Ok("jsonrpc"));
        assert!(check_framing("datagram@3").unwrap_err().contains("`datagram@2`"));
        assert_eq!(DEFAULT_FRAMING, FRAMING_DATAGRAM2);
        assert_eq!(DEFAULT_FORMAT, FORMAT_PACKED);
    }

    #[test]
    fn completion_offers_only_the_canonical_framings() {
        assert_eq!(value_spellings("framing"), vec!["jsonrpc", "datagram", "datagram@2"]);
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

        // A namespaced one isn't official; whether it's declared is checked
        // later, so the namespaced variant lets it through as written.
        assert!(check_handshake("com.acme.hello@1").is_err());
        assert_eq!(check_handshake_or_namespaced("com.acme.hello@1"), Ok("com.acme.hello@1".to_string()));

        for typo in ["comline", "comline@2", "None", "hello@1", "com.acme.hello"] {
            let err = check_handshake_or_namespaced(typo).unwrap_err();
            assert!(err.contains("`comline@1`, `none`"), "{typo}: {err}");
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
