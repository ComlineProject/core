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
//! (A lowered version bump joins when releases do.)
//!
//! The set is frozen beside the reason ([`freeze`]), so a consumer's
//! acceptance and the version comparison see exactly what was given up.

use crate::encodings::{official, EncodingKind};
use crate::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use crate::schema::idl::annotations::{self, DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE, HANDSHAKE_NONE};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};

/// The annotation key.
pub const KEY: &str = "unverified";

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
    /// What a package's context sees: its config's declarations and wire,
    /// and its dependencies' declarations.
    #[cfg(feature = "compiler")]
    pub fn of(context: &'a crate::package::config::ir::context::ProjectContext) -> Self {
        let own = context.config_frozen.as_deref().unwrap_or(&[]);
        let dependencies = context.dependency_configs.iter().flat_map(|(_, units)| units);
        let (format, framing) = crate::package::config::ir::frozen::wire(own);
        Encodings {
            declared: own.iter().chain(dependencies).collect(),
            format,
            framing,
            complete: true,
        }
    }

    /// The declaration of `name` as a `kind`: the package's, a
    /// dependency's, or an official one.
    fn find(&self, kind: EncodingKind, name: &str) -> Option<&'a ConfigUnit> {
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

/// The handshake a protocol's parameters select, canonical.
pub fn handshake(parameters: &[FrozenUnit]) -> &str {
    parameters
        .iter()
        .find_map(|p| match p {
            FrozenUnit::Property { name, value: ConfigValue::Str(v) } if name == "handshake" => {
                Some(annotations::canonical_value("handshake", v).unwrap_or(v.as_str()))
            }
            _ => None,
        })
        .unwrap_or(DEFAULT_HANDSHAKE)
}

/// The framing a protocol uses, as written: its own `@framing` (either
/// form), else the package's.
fn framing<'a>(parameters: &'a [FrozenUnit], encodings: &Encodings<'a>) -> &'a str {
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
        .unwrap_or(DEFAULT_FRAMING)
}

/// The guarantees a protocol with these parameters gives up, whether or not
/// it's marked, each with why. Sorted by guarantee; a guarantee may appear
/// once per cause.
pub fn causes(parameters: &[FrozenUnit], encodings: &Encodings) -> Vec<(Guarantee, String)> {
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
            causes.push((guarantee, why));
        }
    }

    let format = encodings.format.unwrap_or(DEFAULT_FORMAT);
    let framing = framing(parameters, encodings);
    for (kind, name) in [(EncodingKind::Format, format), (EncodingKind::Framing, framing)] {
        if official::resolve(kind, name).is_some() {
            continue;
        }
        let has_vectors = match encodings.find(kind, name) {
            Some(ConfigUnit::Encoding { has_vectors, .. }) => *has_vectors,
            None => !encodings.complete,
            Some(_) => false,
        };
        if !has_vectors {
            causes.push((
                Guarantee::EncodingConformance,
                format!("{} `{name}` ships no conformance vectors", kind.as_str()),
            ));
        }
    }

    causes.sort_by_key(|(guarantee, _)| *guarantee);
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

/// Freeze the given-up set beside every marked protocol's `@unverified`.
/// Run once the protocol's parameters are final (after the package's
/// default handshake is applied). An unmarked protocol gets nothing: giving
/// something up without the marker is a validation error, not a freeze.
pub fn freeze(units: &mut [FrozenUnit], encodings: &Encodings) {
    for unit in units {
        let FrozenUnit::Protocol { parameters, .. } = unit else {
            continue;
        };
        parameters.retain(|p| !matches!(p, FrozenUnit::Unverified(_)));
        if marker(parameters).is_none() {
            continue;
        }
        let set = given_up(parameters, encodings);
        if !set.is_empty() {
            parameters.push(FrozenUnit::Unverified(set));
        }
    }
}

/// Every protocol in `units` that's frozen as unverified: name, set, reason.
pub fn protocols(units: &[FrozenUnit]) -> Vec<(&str, &[Guarantee], Option<&str>)> {
    units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Protocol { name, parameters, .. } => {
                let set = frozen_set(parameters);
                (!set.is_empty()).then(|| (name.as_str(), set, reason(parameters)))
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
