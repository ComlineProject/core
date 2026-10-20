//! `@unverified`: a protocol giving up guarantees Comline otherwise gives
//! its peers, with a reason.
//!
//! The author writes only the marker and the reason; core works out which
//! [`Guarantee`]s are given up, from what it already knows. Today that's the
//! handshake: `none` establishes neither schema nor encoding agreement. (An
//! encoding without conformance vectors and a lowered version bump join when
//! third-party encodings and releases exist.)
//!
//! The set is frozen beside the reason ([`freeze`]), so a consumer's
//! acceptance and the version comparison see exactly what was given up.

use crate::schema::idl::annotations::{self, DEFAULT_HANDSHAKE, HANDSHAKE_NONE};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};

/// The annotation key.
pub const KEY: &str = "unverified";

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

/// The guarantees a protocol with these parameters gives up, whether or not
/// it's marked. Sorted.
pub fn given_up(parameters: &[FrozenUnit]) -> Vec<Guarantee> {
    if handshake(parameters) == HANDSHAKE_NONE {
        vec![Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    } else {
        vec![]
    }
}

/// Why the given-up set is what it is, for a diagnostic: `handshake `none``.
pub fn cause(parameters: &[FrozenUnit]) -> String {
    format!("handshake `{}`", handshake(parameters))
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
pub fn freeze(units: &mut [FrozenUnit]) {
    for unit in units {
        let FrozenUnit::Protocol { parameters, .. } = unit else {
            continue;
        };
        parameters.retain(|p| !matches!(p, FrozenUnit::Unverified(_)));
        if marker(parameters).is_none() {
            continue;
        }
        let set = given_up(parameters);
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
