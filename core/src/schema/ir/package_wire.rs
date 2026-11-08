//! The package's wire choices, frozen into each protocol's IR.
//!
//! A protocol that writes no `@framing` takes the package's `wire.framing`
//! (else [`DEFAULT_FRAMING`](crate::schema::idl::annotations::DEFAULT_FRAMING)); one that writes no `@handshake` takes the
//! package's `handshake` (else [`package_default_handshake`], one for the
//! whole package). Both are frozen into the protocol's parameters, so the
//! choice is part of each protocol's IR and hash and every consumer reads it
//! from the one place.
//!
//! The compiler does this when it compiles a package; an editor or the
//! playground, which interpret schemas on their own, call
//! [`apply_package_wire`] to get the same IR. Both go through
//! [`PackageWire`], so the two can't drift.

use std::collections::BTreeMap;

use crate::package::config::ir::frozen::{self as config, FrozenUnit as ConfigUnit};
use crate::schema::idl::annotations::{package_default_handshake, IMPLICIT_FRAMING, IMPLICIT_HANDSHAKE};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

/// The framing and handshake a package gives its protocols that don't write
/// their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageWire {
    /// The package's `wire.framing`, else [`DEFAULT_FRAMING`](crate::schema::idl::annotations::DEFAULT_FRAMING) (canonical).
    pub framing: String,
    /// The package's `handshake`, else [`package_default_handshake`].
    pub handshake: String,
    /// The parameters written with the package's `handshake`. Empty for the
    /// plain form, and always for the default.
    pub handshake_params: BTreeMap<String, String>,
}

impl PackageWire {
    /// The package's wire, from its frozen config and the written
    /// parameters (annotations) of every protocol it compiles. The
    /// protocols decide the default handshake: `comline@2` only where every
    /// one is on `datagram@3` and names no other handshake.
    pub fn of<'a>(
        config: &[ConfigUnit],
        protocols: impl IntoIterator<Item = &'a [FrozenUnit]>,
    ) -> Self {
        let framing = config::wire_framing(config).to_string();
        let handshake = match config::stated_handshake(config) {
            Some(handshake) => handshake.to_string(),
            None => package_default_handshake(protocols, &framing).to_string(),
        };
        PackageWire {
            framing,
            handshake,
            handshake_params: config::handshake_params(config),
        }
    }

    /// Freeze the handshake into every protocol of `units` that doesn't
    /// name its own. `comline@1` without parameters is what an absent
    /// `@handshake` has always meant, so it's left implicit and the IR of a
    /// package that states it doesn't move. Parameters freeze in
    /// `@handshake`'s dictionary form, as if the protocol had written it;
    /// without any it's the plain name.
    pub fn freeze_handshake(&self, units: &mut [FrozenUnit]) {
        if self.handshake == IMPLICIT_HANDSHAKE && self.handshake_params.is_empty() {
            return;
        }
        let value = if self.handshake_params.is_empty() {
            ConfigValue::Str(self.handshake.clone())
        } else {
            let name = ("name".to_string(), ConfigValue::Str(self.handshake.clone()));
            let params = self
                .handshake_params
                .iter()
                .map(|(k, v)| (k.clone(), ConfigValue::Str(v.clone())));
            ConfigValue::Dict(std::iter::once(name).chain(params).collect())
        };
        freeze_property(units, "handshake", &value);
    }

    /// Freeze the framing into every protocol of `units` that doesn't write
    /// its own `@framing`. Only [`IMPLICIT_FRAMING`], what an absent
    /// `@framing` has always meant, stays implicit: a package that states it
    /// keeps its IR, and one that takes today's default shows the change.
    pub fn freeze_framing(&self, units: &mut [FrozenUnit]) {
        if self.framing == IMPLICIT_FRAMING {
            return;
        }
        freeze_property(units, "framing", &ConfigValue::Str(self.framing.clone()));
    }

    /// [`freeze_handshake`](Self::freeze_handshake), then
    /// [`freeze_framing`](Self::freeze_framing). The compiler freezes
    /// `@unverified` between the two; a caller that freezes it too does the
    /// same, for the same order of parameters.
    pub fn freeze(&self, units: &mut [FrozenUnit]) {
        self.freeze_handshake(units);
        self.freeze_framing(units);
    }
}

/// Freeze the package's framing and handshake into every protocol of
/// `schemas` (all the package's own schemas, interpreted but not compiled)
/// that doesn't write its own, exactly as the compiler does, given the
/// package's frozen config.
pub fn apply_package_wire(schemas: &mut [&mut [FrozenUnit]], config: &[ConfigUnit]) {
    let wire = {
        let protocols = schemas.iter().flat_map(|units| units.iter()).filter_map(|u| match u {
            FrozenUnit::Protocol { parameters, .. } => Some(parameters.as_slice()),
            _ => None,
        });
        PackageWire::of(config, protocols)
    };
    for units in schemas.iter_mut() {
        wire.freeze(units);
    }
}

fn freeze_property(units: &mut [FrozenUnit], key: &str, value: &ConfigValue) {
    for unit in units {
        let FrozenUnit::Protocol { parameters, .. } = unit else {
            continue;
        };
        let declared = parameters
            .iter()
            .any(|p| matches!(p, FrozenUnit::Property { name, .. } if name == key));
        if !declared {
            parameters.push(FrozenUnit::Property { name: key.to_string(), value: value.clone() });
        }
    }
}
