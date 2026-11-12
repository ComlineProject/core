//! `@unverified`: a protocol giving up guarantees Comline otherwise gives
//! its peers, with a reason.
//!
//! The author writes only the marker and the reason; core works out which
//! [`Guarantee`]s are given up, from what it already knows ([`given_up`]):
//!
//! - **The handshake** establishes schema and encoding agreement only where
//!   its profile states them (`schema_agreement`, `encoding_agreement`,
//!   frozen on its declaration as `establishes`). `comline@1`'s states both;
//!   `none` establishes neither; a custom handshake establishes what its
//!   profile states, and an undeclared one nothing.
//! - **The format and framing** keep encoding conformance only when they
//!   ship conformance vectors (`has_vectors` on the declaration). Official
//!   encodings always do.
//!
//! - **`comline@2`'s `same_version_differs = warn`** connects to a peer of the
//!   same version with another shape, so it gives up version honesty. Its other
//!   policies only choose what happens to a difference both ends can see, and
//!   give up nothing.
//!
//! (A lowered version bump joins when releases do.)
//!
//! The set is frozen beside the reason ([`freeze`]), so a consumer's
//! acceptance and the version comparison see exactly what was given up.
//!
//! The reason is one string for everything given up (`@unverified = "..."`),
//! or, keyed, one per guarantee (`@unverified = { bounded_size = "..." }`),
//! which has to say every guarantee the protocol gives up. Either way the
//! frozen set is the one core works out; a keyed form's reasons stay in the
//! frozen annotation.
//!
//! **At package scope.** What a package's settings give up, every protocol
//! that takes them gives up too, so the package can say why once: an
//! `unverified` key in `config.idp`, a reason or, keyed, one per guarantee
//! (`unverified = { schema_agreement = "..." }`), under the same gate as the
//! annotation. It covers only a cause that comes from a package setting, and
//! only for a protocol that takes that setting rather than writing its own
//! (`@handshake`, `@framing`): [`Inherits`]. A guarantee the catalog doesn't
//! give package scope (`bounded_size`) is never covered. A protocol is
//! acknowledged for what its own marker covers and what the package covers
//! for it; the latter is frozen with it, restricted to those guarantees
//! ([`PACKAGE_KEY`]), and the given-up set is still the one core works out.
//!
//! **At inner scopes.** `bounded_size` comes from types, so a marker for it
//! can sit lower than the protocol: on a function, an argument, a struct or
//! error field, or a type declaration. The scopes a guarantee allows are the
//! specification's catalog's (`Guarantee::entry().scopes`); core holds none of
//! its own. A marker at an inner scope acknowledges the unbounded causes
//! reached *through that element only*, and never subtracts from what is
//! computed: a cause no marker covers is still an error. Each acknowledgement
//! is frozen with the scope path it was made at ([`SCOPES_KEY`],
//! [`Acknowledgement`]), as additive data on the protocol.

use crate::encodings::{official, EncodingKind};
use crate::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use crate::schema::idl::annotations::{self, DEFAULT_FORMAT, DEFAULT_FRAMING, FORMAT_JSON, HANDSHAKE_NONE, IMPLICIT_FRAMING, IMPLICIT_HANDSHAKE};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
use comline_catalog::fact;

/// The annotation key.
pub const KEY: &str = "unverified";

/// The property a protocol the package's `unverified` covers carries, written
/// by the compiler beside the given-up set: the package's reasons, as a
/// dictionary from guarantee to reason, for the guarantees it covers for that
/// protocol. A protocol it covers nothing for has none.
pub const PACKAGE_KEY: &str = "unverified_package";

/// The property a protocol carries, written by the compiler beside the
/// given-up set: each acknowledged guarantee with the scope it was
/// acknowledged at and the reason, as a dictionary from guarantee to a
/// dictionary from `"<scope> <path>"` (`"argument Billing.upload(data)"`) to
/// reason. Read it with [`acknowledgements`]. A `SCOPES_KEY` a schema writes
/// itself is dropped: only the compiler says where something was acknowledged.
pub const SCOPES_KEY: &str = "unverified_scopes";

/// Where one guarantee given up was acknowledged, and why: the scope it was
/// written at (the catalog's), the path to the element, and its reason.
///
/// Paths: a protocol `Billing`, a function `Billing.upload`, an argument
/// `Billing.upload(data)`, a field `Upload.data`, a type `Blob` (a type of
/// another schema of the package, by its schema: `models::Blob`), and the
/// package itself an empty path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acknowledgement {
    pub guarantee: Guarantee,
    pub scope: comline_catalog::Scope,
    pub path: String,
    pub reason: String,
}

impl Acknowledgement {
    fn key(&self) -> String {
        format!("{} {}", self.scope.as_str(), self.path)
    }

    fn rank(&self) -> (Guarantee, u8, &str) {
        use comline_catalog::Scope;
        let scope = match self.scope {
            Scope::Package => 0,
            Scope::Protocol => 1,
            Scope::Function => 2,
            Scope::Argument => 3,
            Scope::Field => 4,
            Scope::Type => 5,
        };
        (self.guarantee, scope, self.path.as_str())
    }
}

/// The catalog scope written `name`.
pub fn scope_named(name: &str) -> Option<comline_catalog::Scope> {
    use comline_catalog::Scope;
    [Scope::Package, Scope::Protocol, Scope::Function, Scope::Argument, Scope::Field, Scope::Type]
        .into_iter()
        .find(|scope| scope.as_str() == name)
}

/// What the frozen protocol says was acknowledged, and where
/// ([`SCOPES_KEY`]), sorted by guarantee, then outermost scope first.
pub fn acknowledgements(parameters: &[FrozenUnit]) -> Vec<Acknowledgement> {
    let mut found = vec![];
    let record = parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value: ConfigValue::Dict(entries) } if name == SCOPES_KEY => Some(entries),
        _ => None,
    });
    for (guarantee, at) in record.into_iter().flatten() {
        let (Some(guarantee), ConfigValue::Dict(at)) = (Guarantee::from_name(guarantee), at) else { continue };
        for (key, reason) in at {
            let (Some((scope, path)), ConfigValue::Str(reason)) = (key.split_once(' '), reason) else { continue };
            let Some(scope) = scope_named(scope) else { continue };
            found.push(Acknowledgement { guarantee, scope, path: path.to_string(), reason: reason.clone() });
        }
    }
    found.sort_by(|a, b| a.rank().cmp(&b.rank()));
    found
}

/// Add `more` to what the protocol's `parameters` record as acknowledged.
pub fn acknowledge(parameters: &mut Vec<FrozenUnit>, more: impl IntoIterator<Item = Acknowledgement>) {
    let mut all = acknowledgements(parameters);
    for ack in more {
        if !all.iter().any(|a| a.guarantee == ack.guarantee && a.scope == ack.scope && a.path == ack.path) {
            all.push(ack);
        }
    }
    parameters.retain(|p| !matches!(p, FrozenUnit::Property { name, .. } if name == SCOPES_KEY));
    if all.is_empty() {
        return;
    }
    all.sort_by(|a, b| a.rank().cmp(&b.rank()));
    let mut record: Vec<(String, ConfigValue)> = vec![];
    for ack in &all {
        let entry = (ack.key(), ConfigValue::Str(ack.reason.clone()));
        match record.iter_mut().find(|(g, _)| g == ack.guarantee.name()) {
            Some((_, ConfigValue::Dict(at))) => at.push(entry),
            _ => record.push((ack.guarantee.name().to_string(), ConfigValue::Dict(vec![entry]))),
        }
    }
    parameters.push(FrozenUnit::Property { name: SCOPES_KEY.to_string(), value: ConfigValue::Dict(record) });
}

/// What a protocol takes from the package's settings rather than writing
/// itself, decided before the package's handshake and framing are frozen into
/// it ([`inherits`]): once they are, they read as the protocol's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inherits {
    /// It writes no `@handshake`: the package's, or its default, applies.
    pub handshake: bool,
    /// It writes no `@framing`: the package's, or its default, applies.
    pub framing: bool,
}

/// What the guarantee check reads besides a protocol's own parameters: the
/// encoding declarations it can see (the package's own and its
/// dependencies'; the official ones are always seen), and the package's
/// `wire` defaults for a protocol that doesn't name its own.
///
/// [`Encodings::of`] a package is complete: a name it can't find is
/// undeclared, which establishes nothing and has no vectors. The default
/// knows only the official encodings and is incomplete: a name it can't
/// find is skipped, since it can't say (an editor's view, or the IR on its
/// own). The build always judges with a complete one.
#[derive(Debug, Clone, Default)]
pub struct Encodings<'a> {
    /// `FrozenUnit::Encoding` declarations, own and dependencies'.
    pub declared: Vec<&'a ConfigUnit>,
    /// The package's `wire.format`, if it declares one.
    pub format: Option<&'a str>,
    /// The package's `wire.framing`, if it declares one.
    pub framing: Option<&'a str>,
    /// Whether `declared` holds every declaration the package can see.
    pub complete: bool,
}

impl<'a> Encodings<'a> {
    /// What the frozen configs say: `own`'s declarations and wire, and the
    /// declarations of `dependencies`' (each one's whole frozen config).
    /// `complete` is whether those are every config the package has: pass
    /// `false` where one couldn't be had (a stored version that didn't keep
    /// it, core#251), and a name that isn't found reads as unknown, never as
    /// undeclared. `framing` is `own`'s `wire.framing` as written, `None`
    /// where it states none.
    pub fn from_config(
        own: &'a [ConfigUnit],
        dependencies: impl IntoIterator<Item = &'a [ConfigUnit]>,
        complete: bool,
    ) -> Self {
        let (format, framing) = crate::package::config::ir::frozen::wire(own);
        Encodings {
            declared: own.iter().chain(dependencies.into_iter().flatten()).collect(),
            format,
            framing,
            complete,
        }
    }

    /// What a package's context sees: its config's declarations and wire,
    /// and its dependencies' declarations.
    #[cfg(feature = "compiler")]
    pub fn of(context: &'a crate::package::config::ir::context::ProjectContext) -> Self {
        let own = context.config_frozen.as_deref().unwrap_or(&[]);
        let dependencies = context.dependency_configs.iter().map(|(_, units)| units.as_slice());
        let mut encodings = Self::from_config(own, dependencies, true);
        // What the package compiles to: core's default where it states none.
        encodings.framing = Some(encodings.framing.unwrap_or(DEFAULT_FRAMING));
        encodings
    }

    /// The declaration of `name` as a `kind`: the package's, a
    /// dependency's, or an official one.
    pub fn find(&self, kind: EncodingKind, name: &str) -> Option<&'a ConfigUnit> {
        let declares = |unit: &&ConfigUnit| {
            matches!(unit, ConfigUnit::Encoding { name: n, kind: k, .. } if n == name && *k == kind)
        };
        self.declared
            .iter()
            .copied()
            .find(declares)
            .or_else(|| official::units().iter().find(declares))
    }
}

/// The grammar `name` writes in or into ([`Profile::grammar`]), or `None` if it
/// has none or isn't known here.
///
/// [`Profile::grammar`]: crate::encodings::Profile::grammar
fn grammar(kind: EncodingKind, name: &str, encodings: &Encodings) -> Option<String> {
    if let Some(profile) = official::profile(kind, name) {
        return profile.grammar();
    }
    match encodings.find(kind, name) {
        Some(ConfigUnit::Encoding { grammar, .. }) => grammar.clone(),
        _ => None,
    }
}

/// What the framing `name` states as `fact`, or `None` if it isn't known here.
fn framing_fact(name: &str, fact: &str, encodings: &Encodings) -> Option<String> {
    if let Some(profile) = official::profile(EncodingKind::Framing, name) {
        return profile.stated(fact);
    }
    match encodings.find(EncodingKind::Framing, name) {
        Some(ConfigUnit::Encoding { calls, directions, .. }) => match fact {
            fact::CALLS => calls.clone(),
            fact::DIRECTIONS => directions.clone(),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the framing `name` names its calls (`calls = by_name`), which is
/// what `method_names` chooses how to do. One that isn't known counts only when
/// these encodings can't say.
pub fn names_calls(name: &str, encodings: &Encodings) -> bool {
    match framing_fact(name, fact::CALLS, encodings) {
        Some(calls) => calls == "by_name",
        None => !encodings.complete,
    }
}

/// Whether the framing `name` carries calls one way only (`directions = one`):
/// its frames don't say whether they're a request or a reply.
pub fn one_way_only(name: &str, encodings: &Encodings) -> bool {
    framing_fact(name, fact::DIRECTIONS, encodings).as_deref() == Some("one")
}

/// The format a package gets when it sets none, on the framing `name`: the one
/// writing the grammar the framing writes a payload into, else core's default.
pub fn default_format(name: &str, encodings: &Encodings) -> &'static str {
    match grammar(EncodingKind::Framing, name, encodings).as_deref() {
        Some("json") => FORMAT_JSON,
        _ => DEFAULT_FORMAT,
    }
}

/// [`default_format`], or `None` where it can't be told: the framing `name`
/// isn't one of the official ones and these encodings don't hold its
/// declaration, and they aren't complete, so a declaration that writes JSON
/// may be where they can't see it.
pub fn default_format_known(name: &str, encodings: &Encodings) -> Option<&'static str> {
    let known = official::profile(EncodingKind::Framing, name).is_some()
        || encodings.find(EncodingKind::Framing, name).is_some();
    (known || encodings.complete).then(|| default_format(name, encodings))
}

/// The framing, the grammar it writes payloads into and the format of a
/// protocol that can't work: a framing that writes a payload into a grammar
/// (JSON-RPC's is JSON), with a format that writes something else. A framing
/// that carries bytes takes any format. This goes by the encodings' facts, so a
/// third party's are held to the same rule, and a package that sets no format
/// gets its framing's default, which fits.
pub fn misfit<'a>(parameters: &'a [FrozenUnit], encodings: &Encodings<'a>) -> Option<(&'a str, String, &'a str)> {
    let format = encodings.format?;
    let framing = framing(parameters, encodings);
    let needs = grammar(EncodingKind::Framing, framing, encodings)?;
    let declared = official::resolve(EncodingKind::Format, format).is_some()
        || encodings.find(EncodingKind::Format, format).is_some();
    let writes = grammar(EncodingKind::Format, format, encodings);
    (declared && writes.as_deref() != Some(needs.as_str())).then_some((framing, needs, format))
}

/// The handshake a protocol's parameters select, canonical.
pub fn handshake(parameters: &[FrozenUnit]) -> &str {
    parameters
        .iter()
        .find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == "handshake" => {
                let ConfigValue::Str(v) = (match value {
                    ConfigValue::Dict(_) => value.entry("name")?,
                    other => other,
                }) else {
                    return None;
                };
                Some(annotations::canonical_value("handshake", v).unwrap_or(v.as_str()))
            }
            _ => None,
        })
        .unwrap_or(IMPLICIT_HANDSHAKE)
}

/// The framing a protocol uses, as written: its own `@framing` (either
/// form), else the package's. A compiled protocol carries it unless it's
/// [`annotations::IMPLICIT_FRAMING`], what an absent one has always meant.
pub fn framing<'a>(parameters: &'a [FrozenUnit], encodings: &Encodings<'a>) -> &'a str {
    parameters
        .iter()
        .find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == "framing" => match value {
                ConfigValue::Str(v) => Some(v.as_str()),
                ConfigValue::Dict(_) => match value.entry("name") {
                    Some(ConfigValue::Str(v)) => Some(v.as_str()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .or(encodings.framing)
        .unwrap_or(IMPLICIT_FRAMING)
}

/// Why `bounded_size` is given up: worked out from the types a protocol
/// reaches, so [`causes`] doesn't say; only the frozen set does.
pub const UNBOUNDED_CAUSE: &str = "a string, list or nesting it reaches has no maximum";

/// One reason a guarantee is given up.
struct Cause {
    guarantee: Guarantee,
    why: String,
    /// Whether it comes from a package setting the protocol takes, which the
    /// package can acknowledge, and not from something the protocol writes.
    package: bool,
}

/// The guarantees a protocol with these parameters gives up, whether or not
/// it's marked, each with why. Sorted by guarantee; a guarantee may appear
/// once per cause.
pub fn causes(parameters: &[FrozenUnit], encodings: &Encodings) -> Vec<(Guarantee, String)> {
    causes_from(parameters, encodings, Inherits::default()).into_iter().map(|c| (c.guarantee, c.why)).collect()
}

/// [`causes`], each with whether it comes from a package setting
/// `inherits` says the protocol takes. The handshake's, and the same
/// handshake's policies, come from it; the format is only ever the package's;
/// the framing's, from it.
fn causes_from(parameters: &[FrozenUnit], encodings: &Encodings, inherits: Inherits) -> Vec<Cause> {
    let mut causes = vec![];

    let handshake = handshake(parameters);
    const BOTH: &[Guarantee] = &[Guarantee::SchemaAgreement, Guarantee::EncodingAgreement];
    let establishes: &[Guarantee] = if handshake == HANDSHAKE_NONE {
        &[]
    } else {
        match encodings.find(EncodingKind::Handshake, handshake) {
            Some(ConfigUnit::Encoding { establishes, .. }) => establishes,
            _ if !encodings.complete => BOTH,
            _ => &[],
        }
    };
    for guarantee in [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement] {
        if !establishes.contains(&guarantee) {
            let why = if handshake == HANDSHAKE_NONE {
                format!("handshake `{handshake}`")
            } else {
                format!("handshake `{handshake}` doesn't establish it")
            };
            causes.push(Cause { guarantee, why, package: inherits.handshake });
        }
    }

    if annotations::HandshakeValue::of_protocol(parameters).param("same_version_differs") == Some("warn") {
        let why = format!("handshake `{handshake}` with `same_version_differs = warn`");
        causes.push(Cause { guarantee: Guarantee::VersionHonesty, why, package: inherits.handshake });
    }

    let format = encodings.format.unwrap_or(DEFAULT_FORMAT);
    let framing = framing(parameters, encodings);
    for (kind, name, package) in [
        (EncodingKind::Format, format, true),
        (EncodingKind::Framing, framing, inherits.framing),
    ] {
        if official::resolve(kind, name).is_some() {
            continue;
        }
        let has_vectors = match encodings.find(kind, name) {
            Some(ConfigUnit::Encoding { has_vectors, .. }) => *has_vectors,
            None => !encodings.complete,
            Some(_) => false,
        };
        if !has_vectors {
            causes.push(Cause {
                guarantee: Guarantee::EncodingConformance,
                why: format!("{} `{name}` ships no conformance vectors", kind.as_str()),
                package,
            });
        }
    }

    causes.sort_by_key(|cause| cause.guarantee);
    causes
}

/// The guarantees a protocol with these parameters gives up, whether or not
/// it's marked. Sorted.
pub fn given_up(parameters: &[FrozenUnit], encodings: &Encodings) -> Vec<Guarantee> {
    let mut set: Vec<Guarantee> = causes(parameters, encodings).into_iter().map(|(g, _)| g).collect();
    set.dedup();
    set
}

/// Why the given-up set is what it is, for a diagnostic: `handshake
/// `none``, or each cause, joined. With nothing given up, the handshake.
///
/// This **recomputes** the causes from the encodings it is given, so it is only
/// as good as that view: with [`Encodings::default`] it can't see a
/// dependency's or the package's declarations, and it never says
/// `bounded_size` (see [`UNBOUNDED_CAUSE`]). It is for a build judging a
/// protocol it is compiling. Anything that renders what a protocol was frozen
/// as giving up, a generator or a tool reading a stored version, takes
/// [`frozen_reasons`] instead.
pub fn cause(parameters: &[FrozenUnit], encodings: &Encodings) -> String {
    let mut why: Vec<String> = vec![];
    for (_, cause) in causes(parameters, encodings) {
        if !why.contains(&cause) {
            why.push(cause);
        }
    }
    if why.is_empty() {
        return format!("handshake `{}`", handshake(parameters));
    }
    why.join("; ")
}

/// Why `guarantee` is given up, for its own diagnostic.
pub fn cause_of(parameters: &[FrozenUnit], encodings: &Encodings, guarantee: Guarantee) -> String {
    let why: Vec<String> = causes(parameters, encodings)
        .into_iter()
        .filter(|(g, _)| *g == guarantee)
        .map(|(_, cause)| cause)
        .collect();
    why.join("; ")
}

/// The `@unverified` value, as written.
pub fn marker(parameters: &[FrozenUnit]) -> Option<&ConfigValue> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == KEY => Some(value),
        _ => None,
    })
}

/// The `@unverified` reason, if the marker carries a string.
pub fn reason(parameters: &[FrozenUnit]) -> Option<&str> {
    match marker(parameters) {
        Some(ConfigValue::Str(reason)) => Some(reason),
        _ => None,
    }
}

/// Why `guarantee` is given up, as the author wrote it: the plain form's
/// reason, or the keyed form's entry for it. A guarantee the marker doesn't
/// say is the package's reason for it, where the package acknowledges it for
/// this protocol ([`package_reason_for`]).
pub fn reason_for(parameters: &[FrozenUnit], guarantee: Guarantee) -> Option<&str> {
    marker(parameters)
        .and_then(|marker| reason_in(marker, guarantee))
        .or_else(|| package_reason_for(parameters, guarantee))
}

/// Why `guarantee` is given up, from wherever it was acknowledged: the
/// protocol's own marker or the package's ([`reason_for`]), else the markers
/// at inner scopes that acknowledged it ([`acknowledgements`]), their distinct
/// reasons joined.
pub fn reason_of(parameters: &[FrozenUnit], guarantee: Guarantee) -> Option<String> {
    if let Some(reason) = reason_for(parameters, guarantee) {
        return Some(reason.to_string());
    }
    let mut reasons: Vec<String> = vec![];
    for ack in acknowledgements(parameters).into_iter().filter(|a| a.guarantee == guarantee) {
        if !reasons.contains(&ack.reason) {
            reasons.push(ack.reason);
        }
    }
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

/// `guarantee`'s reason in an acknowledgement as written: the plain form's, or
/// the keyed form's entry for it.
fn reason_in(value: &ConfigValue, guarantee: Guarantee) -> Option<&str> {
    match value {
        ConfigValue::Str(reason) => Some(reason),
        ConfigValue::Dict(entries) => entries.iter().find_map(|(key, value)| match value {
            ConfigValue::Str(reason) if key == guarantee.name() => Some(reason.as_str()),
            _ => None,
        }),
        _ => None,
    }
}

/// What the package's `unverified` covers for this protocol, as frozen
/// ([`PACKAGE_KEY`]): a dictionary from guarantee to reason.
pub fn package_scope(parameters: &[FrozenUnit]) -> Option<&ConfigValue> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == PACKAGE_KEY => Some(value),
        _ => None,
    })
}

/// The package's reason for `guarantee` for this protocol, if its `unverified`
/// covers it there.
pub fn package_reason_for(parameters: &[FrozenUnit], guarantee: Guarantee) -> Option<&str> {
    package_scope(parameters).and_then(|scope| reason_in(scope, guarantee))
}

/// Why the build recorded a guarantee as given up, when it recorded no reason
/// for it: what's known about it without the encodings. `bounded_size` is
/// worked out from the types a protocol reaches and is only in the frozen set,
/// so the one cause there is for it is [`UNBOUNDED_CAUSE`]. The others'
/// causes depend on declarations the frozen protocol doesn't hold.
pub fn unrecorded_cause(guarantee: Guarantee) -> Option<&'static str> {
    (guarantee == Guarantee::BoundedSize).then_some(UNBOUNDED_CAUSE)
}

/// What the frozen protocol says why it gives up each guarantee in its
/// frozen set ([`frozen_set`]), in that order: the reason the build recorded
/// for it ([`reason_for`]: the plain form's, the keyed form's entry or the
/// package's), else [`unrecorded_cause`], else `None`. Nothing here is
/// recomputed from the encodings: the same protocol reads the same wherever
/// it's read. An empty set gives an empty list.
pub fn frozen_reasons(parameters: &[FrozenUnit]) -> Vec<(Guarantee, Option<String>)> {
    frozen_set(parameters)
        .iter()
        .map(|&g| {
            let why = reason_of(parameters, g).or_else(|| unrecorded_cause(g).map(str::to_string));
            (g, why)
        })
        .collect()
}

/// The reason for a diagnostic or an acceptance prompt about `set`: the plain
/// form's, quoted, or the keyed form's, each guarantee with its own
/// (`schema agreement "a", encoding agreement "b"`).
pub fn reason_text(parameters: &[FrozenUnit], set: &[Guarantee]) -> Option<String> {
    if let Some(reason) = reason(parameters) {
        return Some(format!("\"{reason}\""));
    }
    let each: Vec<String> = set
        .iter()
        .filter_map(|g| reason_of(parameters, *g).map(|reason| format!("{} \"{reason}\"", g.describe())))
        .collect();
    (!each.is_empty()).then(|| each.join(", "))
}

/// The frozen given-up set ([`FrozenUnit::Unverified`]); empty when the
/// protocol isn't marked.
pub fn frozen_set(parameters: &[FrozenUnit]) -> &[Guarantee] {
    parameters
        .iter()
        .find_map(|p| match p {
            FrozenUnit::Unverified(set) => Some(set.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

/// For each protocol in `units`, in order, what it takes from the package
/// ([`Inherits`]). Asked of the schema's units as written, before the
/// package's handshake and framing are frozen into them
/// ([`PackageWire`](crate::schema::ir::package_wire::PackageWire)).
pub fn inherits(units: &[FrozenUnit]) -> Vec<Inherits> {
    let writes = |parameters: &[FrozenUnit], key: &str| {
        parameters.iter().any(|p| matches!(p, FrozenUnit::Property { name, .. } if name == key))
    };
    units
        .iter()
        .filter_map(|unit| match unit {
            FrozenUnit::Protocol { parameters, .. } => Some(Inherits {
                handshake: !writes(parameters, "handshake"),
                framing: !writes(parameters, "framing"),
            }),
            _ => None,
        })
        .collect()
}

/// What the package's acknowledgement `package` covers for a protocol with
/// these parameters: each guarantee given up, every cause of which comes from
/// a package setting the protocol takes, and which `package` says a reason
/// for. Sorted by guarantee.
pub fn package_covered(
    parameters: &[FrozenUnit],
    encodings: &Encodings,
    inherits: Inherits,
    package: &ConfigValue,
) -> Vec<(Guarantee, String)> {
    let causes = causes_from(parameters, encodings, inherits);
    let mut covered: Vec<(Guarantee, String)> = vec![];
    for Cause { guarantee, .. } in &causes {
        let from_package = causes.iter().filter(|c| c.guarantee == *guarantee).all(|c| c.package);
        if !from_package || covered.iter().any(|(g, _)| g == guarantee) {
            continue;
        }
        if let Some(reason) = reason_in(package, *guarantee) {
            covered.push((*guarantee, reason.to_string()));
        }
    }
    covered
}

/// Whether `marker` (a plain reason or the keyed form) acknowledges
/// `guarantee`.
pub fn acknowledges(marker: &ConfigValue, guarantee: Guarantee) -> bool {
    reason_in(marker, guarantee).is_some()
}

/// The guarantees a protocol with these parameters gives up only because of
/// package settings it takes (`inherits`), each with why: where the package's
/// own acknowledgement belongs ([`package_covered`]), the innermost scope of
/// what a package setting gives up. Sorted by guarantee.
pub fn package_caused(parameters: &[FrozenUnit], encodings: &Encodings, inherits: Inherits) -> Vec<(Guarantee, String)> {
    let causes = causes_from(parameters, encodings, inherits);
    let mut found: Vec<(Guarantee, String)> = vec![];
    for Cause { guarantee, .. } in &causes {
        let own: Vec<&Cause> = causes.iter().filter(|c| c.guarantee == *guarantee).collect();
        if !own.iter().all(|c| c.package) || found.iter().any(|(g, _)| g == guarantee) {
            continue;
        }
        let mut why: Vec<&str> = vec![];
        for cause in own {
            if !why.contains(&cause.why.as_str()) {
                why.push(&cause.why);
            }
        }
        found.push((*guarantee, why.join("; ")));
    }
    found
}

/// Every guarantee the package covers for some protocol in `units`.
pub fn package_covered_by(units: &[FrozenUnit]) -> Vec<Guarantee> {
    let mut covered = vec![];
    for unit in units {
        if let FrozenUnit::Protocol { parameters, .. } = unit {
            covered.extend(Guarantee::ALL.into_iter().filter(|g| package_reason_for(parameters, *g).is_some()));
        }
    }
    covered.sort();
    covered.dedup();
    covered
}

/// Freeze the given-up set beside every marked protocol's `@unverified`.
/// Run once the protocol's parameters are final (after the package's
/// default handshake is applied). An unmarked protocol gets nothing: giving
/// something up without the marker is a validation error, not a freeze.
pub fn freeze(units: &mut [FrozenUnit], encodings: &Encodings) {
    freeze_with(units, encodings, None, &[]);
}

/// [`freeze`], for a package whose `config.idp` says `package` (its
/// `unverified`). A protocol the package covers something for is frozen like a
/// marked one, its set beside [`PACKAGE_KEY`] holding the package's reasons
/// for what it covers. `inherits` is [`inherits`] of the same units, taken
/// before the package's wire was frozen in; without it nothing is the
/// package's to cover. A `PACKAGE_KEY` the schema wrote itself is dropped:
/// only the compiler says what the package covers.
pub fn freeze_with(
    units: &mut [FrozenUnit],
    encodings: &Encodings,
    package: Option<&ConfigValue>,
    inherits: &[Inherits],
) {
    let mut inherits = inherits.iter();
    for unit in units {
        let FrozenUnit::Protocol { name, parameters, .. } = unit else {
            continue;
        };
        let inherited = inherits.next().copied().unwrap_or_default();
        parameters.retain(|p| {
            !matches!(p, FrozenUnit::Unverified(_))
                && !matches!(p, FrozenUnit::Property { name, .. } if name == PACKAGE_KEY || name == SCOPES_KEY)
        });
        let covered = match package {
            Some(package) => package_covered(parameters, encodings, inherited, package),
            None => vec![],
        };
        if marker(parameters).is_none() && covered.is_empty() {
            continue;
        }
        let set = given_up(parameters, encodings);
        if !set.is_empty() {
            parameters.push(FrozenUnit::Unverified(set.clone()));
        }
        // Where each guarantee was acknowledged, for what this knows: the
        // protocol's own marker, else the package's. What the types give up
        // is added by the bounds pass, which knows the scopes below.
        let mut acks = vec![];
        for guarantee in &set {
            if let Some(reason) = marker(parameters).and_then(|marker| reason_in(marker, *guarantee)) {
                let scope = comline_catalog::Scope::Protocol;
                acks.push(Acknowledgement { guarantee: *guarantee, scope, path: name.clone(), reason: reason.to_string() });
            } else if let Some((_, reason)) = covered.iter().find(|(g, _)| g == guarantee) {
                let scope = comline_catalog::Scope::Package;
                acks.push(Acknowledgement { guarantee: *guarantee, scope, path: String::new(), reason: reason.clone() });
            }
        }
        acknowledge(parameters, acks);
        if !covered.is_empty() {
            let reasons = covered.into_iter().map(|(g, reason)| (g.name().to_string(), ConfigValue::Str(reason)));
            parameters.push(FrozenUnit::Property {
                name: PACKAGE_KEY.to_string(),
                value: ConfigValue::Dict(reasons.collect()),
            });
        }
    }
}

/// Every protocol in `units` that's frozen as unverified: name, set, reason
/// ([`reason_text`]).
pub fn protocols(units: &[FrozenUnit]) -> Vec<(&str, &[Guarantee], Option<String>)> {
    units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Protocol { name, parameters, .. } => {
                let set = frozen_set(parameters);
                (!set.is_empty()).then(|| (name.as_str(), set, reason_text(parameters, set)))
            }
            _ => None,
        })
        .collect()
}

/// `schema agreement and encoding agreement`.
pub fn describe(set: &[Guarantee]) -> String {
    let names: Vec<&str> = set.iter().map(|g| g.describe()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `["schema_agreement", "encoding_agreement"]`, as a consumer writes it.
pub fn written(set: &[Guarantee]) -> String {
    let names: Vec<String> = set.iter().map(|g| format!("\"{}\"", g.name())).collect();
    format!("[{}]", names.join(", "))
}
