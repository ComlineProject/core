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

use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::config_value::{DictKey, StrVariant, ValueType};
use crate::encodings::{official, EncodingKind};
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
    /// Inside a function's parentheses, before an argument.
    Argument,
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

// The official formats, framings and handshakes, and their aliases, are
// declared by the embedded official encodings package
// (`crate::encodings::official`), not listed here. The constants below name
// the ones core and the generators act on, by the specification's catalog
// (`comline_catalog::encoding`). `datagram@1` and `jsonrpc@1` are frozen by
// the spellings they had before names were versioned, the catalog's
// `frozen_as`, which a test holds these to.

/// The original datagram framing (`datagram@1`): a request names its call
/// but not its protocol, so a transport carries one protocol, called one
/// way.
pub const FRAMING_DATAGRAM: &str = "datagram";

/// `datagram@2`: the datagram framing whose frames say whether they're a
/// request or a reply, and which protocol they belong to - what calls in
/// both directions need.
pub const FRAMING_DATAGRAM2: &str = comline_catalog::encoding::DATAGRAM_2;

/// `datagram@3`: `datagram@2`'s frames, with calls addressed by wire
/// number rather than position.
pub const FRAMING_DATAGRAM3: &str = comline_catalog::encoding::DATAGRAM_3;

/// JSON-RPC's canonical name.
pub const FRAMING_JSONRPC: &str = "jsonrpc";

/// The framing a protocol gets when neither it (`@framing`) nor its package
/// (`wire.framing`) names one: a connection, so several protocols share a
/// transport and calls go both ways, addressed by wire number.
pub const DEFAULT_FRAMING: &str = FRAMING_DATAGRAM3;

/// What a protocol's frozen IR means when it carries no `@framing`: core's
/// default before `datagram@3`. A protocol that takes today's default has it
/// frozen in, so a version built before the flip reads as it was built, and
/// a package that states `datagram@2` keeps the IR it had.
pub const IMPLICIT_FRAMING: &str = FRAMING_DATAGRAM2;

/// `packed@1`: fixed-width fields back to back, varint lengths, no tags.
pub const FORMAT_PACKED: &str = comline_catalog::encoding::PACKED_1;
/// JSON, a struct as an object.
pub const FORMAT_JSON: &str = comline_catalog::encoding::JSON_1;
/// MessagePack, a struct as an array.
pub const FORMAT_MSGPACK: &str = comline_catalog::encoding::MSGPACK_1;

/// The format a package speaks when its `wire.format` doesn't say.
pub const DEFAULT_FORMAT: &str = FORMAT_PACKED;

/// The official names of `kind`, quoted and comma-separated, as the
/// official package declares them.
fn official_names(kind: EncodingKind) -> String {
    official::names(kind).iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

/// Check a written wire format name against the official ones, returning
/// its canonical spelling.
pub fn check_format(written: &str) -> Result<&'static str, String> {
    official::resolve(EncodingKind::Format, written).ok_or_else(|| {
        format!(
            "wire format `{written}` isn't known: expected one of {}, or a namespaced format \
             this package or a dependency declares under `encodings`",
            official_names(EncodingKind::Format)
        )
    })
}

/// Check a written framing name against the official ones, returning its
/// canonical spelling.
pub fn check_framing(written: &str) -> Result<&'static str, String> {
    official::resolve(EncodingKind::Framing, written).ok_or_else(|| {
        format!(
            "framing `{written}` isn't known: expected one of {}, or a namespaced framing this \
             package or a dependency declares under `encodings`",
            official_names(EncodingKind::Framing)
        )
    })
}

/// How a framing that names its calls does on a connection, `@framing`'s `method_names`:
/// the function alone (`send`), or with its protocol (`chat.Chat/send`).
pub const METHOD_NAMES: &[&str] = &["bare", "qualified"];
const METHOD_NAME_STRINGS: &[StrVariant] = &[StrVariant::new("bare"), StrVariant::new("qualified")];

/// `@framing`'s value: an official framing's name, or the dictionary form,
/// of which the bare name is shorthand for `{ name = ... }`.
fn framing_value() -> ValueType {
    let framings = ValueType::StrEnum(official::variants(EncodingKind::Framing));
    let keys: &'static [DictKey] = Box::leak(Box::new([
        DictKey { key: "name", value: framings, required: true },
        DictKey {
            key: "method_names",
            value: ValueType::AnyOf(&[ValueType::Keyword(METHOD_NAMES), ValueType::StrEnum(METHOD_NAME_STRINGS)]),
            required: false,
        },
    ]));
    ValueType::AnyOf(Box::leak(Box::new([framings, ValueType::Dict(keys)])))
}

/// How a framing that names its calls names them (`@framing`'s `method_names`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MethodNames {
    /// The function's name alone. The default.
    #[default]
    Bare,
    /// `namespace.Protocol/function`.
    Qualified,
}

impl MethodNames {
    /// What `@framing`'s value says: its `method_names`, else the default.
    /// `None` for a value that isn't one, which validation reports.
    pub fn of(value: &ConfigValue) -> Option<MethodNames> {
        match value.entry("method_names") {
            None => Some(MethodNames::Bare),
            Some(ConfigValue::Str(m) | ConfigValue::Name(m)) => match m.as_str() {
                "bare" => Some(MethodNames::Bare),
                "qualified" => Some(MethodNames::Qualified),
                _ => None,
            },
            Some(_) => None,
        }
    }

    /// What a protocol's `@framing` says, of any framing: validation has
    /// already refused `method_names` on one that doesn't name its calls.
    pub fn of_protocol(parameters: &[FrozenUnit]) -> MethodNames {
        parameters
            .iter()
            .find_map(|p| match p {
                FrozenUnit::Property { name, value } if name == "framing" => MethodNames::of(value),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// A protocol's framing, read from either form of `@framing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramingValue {
    /// The canonical name (`jsonrpc`, `datagram`, `datagram@2`).
    pub name: &'static str,
    /// Meaningful under a framing that names its calls only.
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
                Some(FramingValue { method_names: MethodNames::of(value)?, ..FramingValue::named(name)? })
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

/// One parameter a handshake takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandshakeParam {
    pub key: &'static str,
    /// Every value it can have, its default first.
    pub values: &'static [&'static str],
    /// A wire parameter changes what the two ends exchange, so both have to
    /// agree on it and it's part of the handshake's parameter hash. Any other
    /// is a policy, which each end applies on its own.
    pub wire: bool,
}

/// `comline@2`'s parameters (runtime#15).
pub const COMLINE2_PARAMS: &[HandshakeParam] = &[
    HandshakeParam { key: "scope", values: &["protocol", "package"], wire: false },
    HandshakeParam { key: "newer_consumer", values: &["degrade", "refuse"], wire: false },
    HandshakeParam { key: "same_version_differs", values: &["refuse", "warn"], wire: false },
    HandshakeParam { key: "exchange", values: &["offer", "symmetric"], wire: true },
];

/// The parameters the handshake `name` takes. Only `comline@2` takes any: a
/// third party's profile can't declare parameters, as it can't for a framing.
pub fn handshake_params(name: &str) -> &'static [HandshakeParam] {
    match name {
        "comline@2" => COMLINE2_PARAMS,
        _ => &[],
    }
}

/// Check the parameters written for the handshake `name`. The error is a
/// sentence, for `config.idp`'s `handshake` and `@handshake` alike.
pub fn check_handshake_params<'a>(
    name: &str,
    written: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    let params = handshake_params(name);
    let mut seen: Vec<&str> = vec![];
    for (key, value) in written {
        if seen.contains(&key) {
            return Err(format!("handshake parameter `{key}` is set twice"));
        }
        seen.push(key);
        if params.is_empty() {
            return Err(format!("handshake `{name}` takes no parameters, so `{key}` can't be set"));
        }
        let Some(param) = params.iter().find(|p| p.key == key) else {
            let expected: Vec<String> = params.iter().map(|p| format!("`{}`", p.key)).collect();
            return Err(format!(
                "`{key}` isn't a parameter of handshake `{name}`: expected {}",
                expected.join(", ")
            ));
        };
        if !param.values.contains(&value) {
            let expected: Vec<String> = param.values.iter().map(|v| format!("`{v}`")).collect();
            return Err(format!(
                "handshake `{name}`'s `{key}` can't be `{value}`: expected {}",
                expected.join(" or ")
            ));
        }
    }
    Ok(())
}

/// `@handshake`'s value: an official handshake's name, or the dictionary form,
/// of which the bare name is shorthand for `{ name = ... }`. The dictionary's
/// keys are `comline@2`'s; which handshake takes what is
/// [`check_handshake_params`]'s.
fn handshake_value() -> ValueType {
    let handshakes = ValueType::StrEnum(official::variants(EncodingKind::Handshake));
    let mut keys = vec![DictKey { key: "name", value: handshakes, required: true }];
    for param in COMLINE2_PARAMS {
        let strings: Vec<StrVariant> = param.values.iter().map(|v| StrVariant::new(v)).collect();
        let alternatives = [ValueType::Keyword(param.values), ValueType::StrEnum(Box::leak(strings.into_boxed_slice()))];
        keys.push(DictKey { key: param.key, value: ValueType::AnyOf(Box::leak(Box::new(alternatives))), required: false });
    }
    let keys: &'static [DictKey] = Box::leak(keys.into_boxed_slice());
    ValueType::AnyOf(Box::leak(Box::new([handshakes, ValueType::Dict(keys)])))
}

/// `@unverified`'s value: one reason, or a dictionary with a reason for each
/// guarantee given up, keyed by its catalog id. Which keys a protocol has to
/// write is [`crate::schema::ir::unverified`]'s to say, with its own
/// diagnostics, so the dictionary's entries aren't checked here.
fn unverified_value() -> ValueType {
    let keys: Vec<DictKey> = comline_catalog::sets::GUARANTEES
        .iter()
        .map(|id| DictKey { key: *id, value: ValueType::Str, required: false })
        .collect();
    let keys: &'static [DictKey] = Box::leak(keys.into_boxed_slice());
    ValueType::AnyOf(Box::leak(Box::new([ValueType::Str, ValueType::Dict(keys)])))
}

/// A protocol's handshake, read from either form of `@handshake`. A parameter
/// written as its default is the same as one left out, so **the two forms, and
/// a default written out, can't be told apart anywhere they mean the same.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeValue {
    /// The canonical name, or a namespaced one as written.
    pub name: String,
    /// Those set to something other than their default.
    pub params: BTreeMap<String, String>,
}

impl HandshakeValue {
    /// `name`, with its parameters at their defaults.
    pub fn named(name: &str) -> HandshakeValue {
        HandshakeValue { name: canonical_value("handshake", name).unwrap_or(name).to_string(), params: BTreeMap::new() }
    }

    /// `name`, with the parameters written.
    pub fn with<'a>(name: &str, written: impl IntoIterator<Item = (&'a str, &'a str)>) -> HandshakeValue {
        let mut value = HandshakeValue::named(name);
        let known = handshake_params(&value.name);
        for (key, written) in written {
            let default = known.iter().find(|p| p.key == key).and_then(|p| p.values.first());
            if default != Some(&written) {
                value.params.insert(key.to_string(), written.to_string());
            }
        }
        value
    }

    /// `@handshake`'s value, `"name"` or `{ name = ..., k = v }`. `None` for a
    /// value that isn't a handshake, which validation reports.
    pub fn of(value: &ConfigValue) -> Option<HandshakeValue> {
        match value {
            ConfigValue::Str(name) => Some(HandshakeValue::named(name)),
            ConfigValue::Dict(entries) => {
                let ConfigValue::Str(name) = value.entry("name")? else { return None };
                let params = entries.iter().filter(|(k, _)| k != "name").filter_map(|(k, v)| match v {
                    ConfigValue::Str(v) | ConfigValue::Name(v) => Some((k.as_str(), v.as_str())),
                    _ => None,
                });
                Some(HandshakeValue::with(name, params))
            }
            _ => None,
        }
    }

    /// The handshake a protocol's parameters select: its `@handshake`, which
    /// holds the package's or core's default where that isn't
    /// [`IMPLICIT_HANDSHAKE`], else that.
    pub fn of_protocol(parameters: &[FrozenUnit]) -> HandshakeValue {
        parameters
            .iter()
            .find_map(|p| match p {
                FrozenUnit::Property { name, value } if name == "handshake" => HandshakeValue::of(value),
                _ => None,
            })
            .unwrap_or_else(|| HandshakeValue::named(IMPLICIT_HANDSHAKE))
    }

    /// The value of `key`, its default if left out. `None` if this handshake
    /// doesn't take it.
    pub fn param(&self, key: &str) -> Option<&str> {
        let param = handshake_params(&self.name).iter().find(|p| p.key == key)?;
        Some(self.params.get(key).map_or(param.values[0], String::as_str))
    }

    /// What both ends must agree on: the non-default wire parameters. This,
    /// with the name, is what the parameter hash covers.
    pub fn wire_params(&self) -> BTreeMap<String, String> {
        let known = handshake_params(&self.name);
        self.params
            .iter()
            .filter(|(k, _)| known.iter().find(|p| p.key == k.as_str()).is_none_or(|p| p.wire))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Whether the two exchange the same bytes, whatever each end's policies.
    pub fn same_wire(&self, other: &HandshakeValue) -> bool {
        self.name == other.name && self.wire_params() == other.wire_params()
    }
}

/// Which side implements a protocol (`@provider`), or calls it
/// (`@consumer`): the side that dialled (`Client`), the side that accepted
/// (`Server`), or both (`Multiple`).
pub const SIDES: &[&str] = &["Server", "Client", "Multiple"];

/// The handshake a protocol runs when neither it nor its package names one,
/// and the package's protocols are all on [`DEFAULT_FRAMING`]: `comline@2`
/// needs a connection and the wire numbers `datagram@3` carries. Any other
/// package gets [`IMPLICIT_HANDSHAKE`] instead: see
/// [`package_default_handshake`].
pub const DEFAULT_HANDSHAKE: &str = comline_catalog::encoding::COMLINE_2;

/// What a protocol's frozen IR means when it carries no `@handshake`: core's
/// default before `comline@2`, and still the default for a protocol on any
/// framing but `datagram@3`, which may not run on a connection.
pub const IMPLICIT_HANDSHAKE: &str = comline_catalog::encoding::COMLINE_1;

/// The handshake a protocol on `framing` (canonical) gets when neither it
/// nor its package names one: [`DEFAULT_HANDSHAKE`] on [`DEFAULT_FRAMING`],
/// else [`IMPLICIT_HANDSHAKE`]. So writing the default framing out changes
/// nothing, a package stating `datagram@2` keeps the handshake it had, and
/// a lockstep framing never gets a handshake that needs a connection.
pub fn default_handshake(framing: &str) -> &'static str {
    if framing == DEFAULT_FRAMING { DEFAULT_HANDSHAKE } else { IMPLICIT_HANDSHAKE }
}

/// The handshake a package's protocols get when neither they nor the
/// package name one, given each protocol's written annotations and the
/// package's framing (its `wire.framing`, else [`DEFAULT_FRAMING`]).
/// [`DEFAULT_HANDSHAKE`] only when every protocol is on `datagram@3` and
/// names no other handshake: `comline@2` runs one handshake per connection,
/// so a package whose protocols can't share one keeps
/// [`IMPLICIT_HANDSHAKE`] for all of them, as before the defaults moved, and
/// is never refused for taking the defaults.
pub fn package_default_handshake<'a>(
    protocols: impl IntoIterator<Item = &'a [FrozenUnit]>,
    package_framing: &str,
) -> &'static str {
    let property = |parameters: &'a [FrozenUnit], key: &str| {
        parameters.iter().find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == key => Some(value),
            _ => None,
        })
    };
    let all = protocols.into_iter().all(|parameters| {
        let framing = match property(parameters, "framing") {
            Some(value) => FramingValue::of(value).map(|f| f.name),
            None => canonical_value("framing", package_framing),
        };
        let handshake = property(parameters, "handshake").map(HandshakeValue::of);
        framing == Some(DEFAULT_FRAMING)
            && handshake.is_none_or(|h| h == Some(HandshakeValue::named(DEFAULT_HANDSHAKE)))
    });
    if all { DEFAULT_HANDSHAKE } else { IMPLICIT_HANDSHAKE }
}
/// The official "no handshake": nothing sent, nothing checked.
pub const HANDSHAKE_NONE: &str = comline_catalog::encoding::NONE;

/// Check a written handshake name against the official ones, returning
/// its canonical spelling. A namespaced name (`com.acme.hello@1`) is a
/// declared one, which [`check_handshake_or_namespaced`] lets through.
pub fn check_handshake(written: &str) -> Result<&'static str, String> {
    official::resolve(EncodingKind::Handshake, written).ok_or_else(|| {
        format!(
            "handshake `{written}` isn't known: expected {}, or a namespaced handshake this \
             package or a dependency declares under `encodings`",
            official_names(EncodingKind::Handshake)
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

/// Whether `written` is a spelling of an official encoding of `kind`, one
/// the official package declares.
pub fn is_official(kind: EncodingKind, written: &str) -> bool {
    official::resolve(kind, written).is_some()
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

/// Every known annotation. Built once, on first use: `@framing`'s and
/// `@handshake`'s accepted names come from the official encodings package.
pub static KNOWN_ANNOTATIONS: LazyLock<Vec<AnnotationInfo>> = LazyLock::new(known_annotations);

fn known_annotations() -> Vec<AnnotationInfo> {
    vec![
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
        key: "max_depth",
        scopes: &[AnnotationScope::Leading, AnnotationScope::TypeAlias],
        authority: AnnotationAuthority::Core,
        description: "How deeply a type that can contain itself nests: a decoder refuses a \
                       value nested deeper (core#98). Overrides `bounds.depth.max`.",
        default: "the `bounds.depth.max` setting, where one is set",
        value: ValueType::Integer,
        consumed_by: &["`comline build`'s bounds pass"],
    },
    AnnotationInfo {
        key: "id",
        scopes: &[AnnotationScope::Leading, AnnotationScope::Function],
        authority: AnnotationAuthority::Core,
        description: "This protocol's or function's wire number, written out (a `u16`, unique \
                       among the package's protocols or the protocol's functions). Optional: \
                       every protocol and function gets a number in the version's facts, bound \
                       at `comline release`. An id that history shows belonged to another item \
                       is refused. No effect on a struct.",
        default: "the number assigned in the version's facts",
        value: ValueType::Integer,
        consumed_by: &["`comline build`'s numbering (`comline_core::compat`)"],
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
        default: "the package's `wire.framing`, else `datagram@3` — Comline's compact binary \
                  framing on a connection, calls addressed by wire number (`datagram@2`: by \
                  position, the default before; `datagram@1`: one protocol per transport)",
        value: framing_value(),
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
                       (`@framing = \"datagram@3\"`, the default, or `\"jsonrpc\"`). No effect on a struct.",
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
                       both ends share the schema, format and framing; `comline@2` checks \
                       compatibility by each protocol's line, and takes parameters: \
                       `{ name = \"comline@2\", scope = package }` (`scope`, `newer_consumer`, \
                       `same_version_differs`, `exchange`); `none` sends and checks nothing. \
                       Overrides the package's `handshake` in `config.idp`. No effect on a struct.",
        default: "the package's `handshake`, else `comline@2` when all the package's protocols \
                  are on `datagram@3`, and `comline@1` otherwise",
        value: handshake_value(),
        consumed_by: &["comline-rust's generator", "comline-typescript's generator"],
    },
    AnnotationInfo {
        key: "unverified",
        scopes: &[
            AnnotationScope::Leading,
            AnnotationScope::TypeAlias,
            AnnotationScope::Field,
            AnnotationScope::Function,
            AnnotationScope::Argument,
        ],
        authority: AnnotationAuthority::Core,
        description: "Gives up guarantees Comline otherwise gives this protocol's peers, with \
                       the reason why. Core works out which (a `none` handshake gives up schema \
                       and encoding agreement), and every consumer has to accept them. Required \
                       where something is given up; off by default \
                       (`annotations.unverified.allowed`). One reason for all of it, or a \
                       dictionary with a reason for each guarantee given up, by its id: \
                       `{ bounded_size = \"the uploader streams whatever it's given\" }`, \
                       written `@unverified(bounded_size = \"...\")` as well. Where it's \
                       written is its scope, and each guarantee allows the scopes the \
                       specification's catalog gives it: `bounded_size` down to a function, \
                       argument, field or type, where the marker acknowledges only what is \
                       reached through that element.",
        default: "nothing given up",
        value: unverified_value(),
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
    ]
}

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
    fn the_frozen_spellings_are_the_catalogs() {
        use comline_catalog::{encoding, resolve};
        assert_eq!(FRAMING_DATAGRAM, resolve(encoding::DATAGRAM_1).unwrap().frozen());
        assert_eq!(FRAMING_JSONRPC, resolve(encoding::JSONRPC_1).unwrap().frozen());
    }

    #[test]
    fn no_duplicate_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for a in KNOWN_ANNOTATIONS.iter() {
            assert!(seen.insert(a.key), "duplicate annotation key: {}", a.key);
        }
    }

    #[test]
    fn lookup_and_for_scope_agree_with_the_table() {
        assert!(lookup("validators").is_some());
        assert!(lookup("deprecated").is_some());
        assert!(lookup("nope").is_none());
        // id, timeout_ms, idempotent, unverified (all Function), plus settings
        // (Universal).
        assert_eq!(for_scope(AnnotationScope::Function).count(), 5);
        // unverified, plus settings (Universal).
        let mut argument: Vec<_> = for_scope(AnnotationScope::Argument).map(|a| a.key).collect();
        argument.sort();
        assert_eq!(argument, ["settings", "unverified"]);
        // id, max_depth, framing, provider, consumer, handshake, unverified,
        // deprecated (all Leading), plus settings (Universal).
        assert_eq!(for_scope(AnnotationScope::Leading).count(), 9);
        // validators, deprecated, max_depth, plus settings (Universal).
        let mut alias: Vec<_> = for_scope(AnnotationScope::TypeAlias).map(|a| a.key).collect();
        alias.sort();
        assert_eq!(alias, ["deprecated", "max_depth", "settings", "unverified", "validators"]);
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
        assert_eq!(check_framing("datagram@3"), Ok("datagram@3"));
        assert!(check_framing("datagram@4").unwrap_err().contains("`datagram@2`, `datagram@3`"));
        assert_eq!(DEFAULT_FRAMING, FRAMING_DATAGRAM3);
        assert_eq!(IMPLICIT_FRAMING, FRAMING_DATAGRAM2);
        assert_eq!(DEFAULT_FORMAT, FORMAT_PACKED);
    }

    #[test]
    fn completion_offers_only_the_canonical_framings() {
        assert_eq!(value_spellings("framing"), vec!["jsonrpc", "datagram", "datagram@2", "datagram@3"]);
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
        for info in KNOWN_ANNOTATIONS.iter() {
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
        assert_eq!(check_handshake("comline@2"), Ok("comline@2"));
        assert_eq!(check_handshake("none"), Ok("none"));
        assert_eq!(value_spellings("handshake"), vec!["comline@1", "comline@2", "none"]);

        // A namespaced one isn't official; whether it's declared is checked
        // later, so the namespaced variant lets it through as written.
        assert!(check_handshake("com.acme.hello@1").is_err());
        assert_eq!(check_handshake_or_namespaced("com.acme.hello@1"), Ok("com.acme.hello@1".to_string()));

        for typo in ["comline", "comline@3", "None", "hello@1", "com.acme.hello"] {
            let err = check_handshake_or_namespaced(typo).unwrap_err();
            assert!(err.contains("`comline@1`, `comline@2`, `none`"), "{typo}: {err}");
        }
    }

    #[test]
    fn universal_scope_matches_every_scope() {
        for scope in [
            AnnotationScope::Leading,
            AnnotationScope::TypeAlias,
            AnnotationScope::Field,
            AnnotationScope::Function,
            AnnotationScope::Argument,
        ] {
            assert!(for_scope(scope).any(|a| a.key == "settings"));
        }
    }
}
