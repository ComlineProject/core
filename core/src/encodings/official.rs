//! The official encodings package, `comline`: `packed@1`, `json@1`,
//! `msgpack@1`, `datagram@1`, `datagram@2`, `jsonrpc@1` and `comline@1`,
//! declared under `encodings` in its `config.idp` with their profiles and the
//! official generators' mappings, the same way any package declares its own.
//!
//! It is embedded by `comline-core-stdlib` beside std
//! (`packages/comline`), so every consumer of core (the CLI, the language
//! server, the playground) has it with no files on disk. Every package sees
//! its declarations, as every package sees std, but they are never copied
//! into a package's frozen config: a package that declares nothing freezes
//! exactly as it did before.
//!
//! **The name:** `comline`. The package is never written anywhere (no
//! `use comline::...`, no dependency entry): it is identified by its origin,
//! [`Origin::Official`], not by its name, so a user package that happens to
//! be called `comline` doesn't collide with it and gets no privileges.
//!
//! **Spellings:** a declared name is one spelling. The names written before
//! the declarations existed are kept by [`SPELLINGS`]: a declaration can't
//! state aliases, and only the official encodings have any.

// Standard Uses
use std::sync::OnceLock;

// Crate Uses
use super::EncodingKind;
use crate::config_value::{canonicalize, StrVariant, ValueType};
use crate::package::config::ir::context::Origin;
use crate::package::config::ir::frozen::FrozenUnit;


/// The official package's name, as its `config.idp` declares it.
pub const PACKAGE: &str = "comline";

/// The official names whose canonical spelling isn't the declared name:
/// `(declared name, canonical spelling, other aliases)`. The canonical
/// spelling is what core freezes and what the generators match on
/// (`FRAMING_JSONRPC`, `FRAMING_DATAGRAM`), so it stays the one used before
/// the names were declared, and the IR of every existing package with it.
/// The declared name is accepted as one more alias.
const SPELLINGS: &[(&str, &str, &[&str])] = &[
    ("jsonrpc@1", crate::schema::idl::annotations::FRAMING_JSONRPC, &["json-rpc", "jsonrpc-2.0"]),
    ("datagram@1", crate::schema::idl::annotations::FRAMING_DATAGRAM, &["comline.datagram"]),
];

/// The official package's frozen declarations: one
/// [`FrozenUnit::Encoding`] per official encoding, in the order its
/// `config.idp` declares them.
///
/// Panics if the embedded package doesn't freeze, which a test proves it
/// does (`every_official_encoding_freezes`).
pub fn units() -> &'static [FrozenUnit] {
    static UNITS: OnceLock<Vec<FrozenUnit>> = OnceLock::new();
    UNITS.get_or_init(|| {
        let mut context = crate::package::config::ir::interpreter::ProjectInterpreter::from_config_source(
            comline_core_stdlib::OFFICIAL_MANIFEST,
        )
        .unwrap_or_else(|e| panic!("the official encodings package doesn't parse: {e}"));
        context.origin = Origin::Official;
        // Only `encodings`: the rest of the manifest names the package.
        context
            .config
            .assignments
            .iter()
            .filter(|assignment| assignment.key.text() == "encodings")
            .flat_map(|assignment| {
                crate::package::config::ir::interpreter::freezing::interpret_assignment(&context, assignment, None)
                    .unwrap_or_else(|e| panic!("the official encodings package doesn't freeze: {e}"))
            })
            .collect()
    })
}

/// Every spelling of the official encodings of `kind`, as a closed set:
/// what `@framing`, `@handshake`, `wire` and `handshake` accept without a
/// declaration of their own. Handshakes add `none`, which isn't declared
/// because it is the absence of a handshake.
pub fn variants(kind: EncodingKind) -> &'static [StrVariant] {
    static VARIANTS: OnceLock<[&'static [StrVariant]; 3]> = OnceLock::new();
    let all = VARIANTS.get_or_init(|| {
        [EncodingKind::Format, EncodingKind::Framing, EncodingKind::Handshake].map(|kind| {
            let mut variants: Vec<StrVariant> = super::declared(units())
                .filter(|(_, k)| *k == kind)
                .map(|(name, _)| match SPELLINGS.iter().find(|(declared, ..)| *declared == name) {
                    Some((declared, canonical, aliases)) => {
                        let all: Vec<&'static str> = std::iter::once(*declared).chain(aliases.iter().copied()).collect();
                        StrVariant::with_aliases(canonical, Box::leak(all.into_boxed_slice()))
                    }
                    None => StrVariant::new(name),
                })
                .collect();
            if kind == EncodingKind::Handshake {
                variants.push(StrVariant::new(crate::schema::idl::annotations::HANDSHAKE_NONE));
            }
            &*Box::leak(variants.into_boxed_slice())
        })
    });
    match kind {
        EncodingKind::Format => all[0],
        EncodingKind::Framing => all[1],
        EncodingKind::Handshake => all[2],
    }
}

/// The canonical spelling of an official encoding of `kind` written as
/// `written` (`json-rpc` -> `jsonrpc`), or `None` if it isn't one.
pub fn resolve(kind: EncodingKind, written: &str) -> Option<&'static str> {
    canonicalize(&ValueType::StrEnum(variants(kind)), written)
}

/// The official encodings of `kind`, by canonical spelling, in declaration
/// order: what an error lists as the expected names.
pub fn names(kind: EncodingKind) -> Vec<&'static str> {
    variants(kind).iter().map(|v| v.canonical).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_official_encoding_freezes() {
        let declared: Vec<(&str, EncodingKind)> = super::super::declared(units()).collect();
        assert_eq!(
            declared,
            [
                ("packed@1", EncodingKind::Format),
                ("json@1", EncodingKind::Format),
                ("msgpack@1", EncodingKind::Format),
                ("jsonrpc@1", EncodingKind::Framing),
                ("datagram@1", EncodingKind::Framing),
                ("datagram@2", EncodingKind::Framing),
                ("comline@1", EncodingKind::Handshake),
            ]
        );
        // Each maps onto both official generators, but for MessagePack,
        // which TypeScript has no codec for.
        for unit in units() {
            let FrozenUnit::Encoding { name, map, .. } = unit else { unreachable!() };
            assert!(map.contains_key("rust"), "{name}");
            assert_eq!(map.contains_key("typescript"), name != "msgpack@1", "{name}");
        }
    }

    #[test]
    fn every_name_and_alias_resolves_as_before() {
        let cases = [
            (EncodingKind::Format, "packed@1", "packed@1"),
            (EncodingKind::Format, "json@1", "json@1"),
            (EncodingKind::Format, "msgpack@1", "msgpack@1"),
            (EncodingKind::Framing, "jsonrpc", "jsonrpc"),
            (EncodingKind::Framing, "json-rpc", "jsonrpc"),
            (EncodingKind::Framing, "jsonrpc-2.0", "jsonrpc"),
            (EncodingKind::Framing, "jsonrpc@1", "jsonrpc"),
            (EncodingKind::Framing, "datagram", "datagram"),
            (EncodingKind::Framing, "datagram@1", "datagram"),
            (EncodingKind::Framing, "comline.datagram", "datagram"),
            (EncodingKind::Framing, "datagram@2", "datagram@2"),
            (EncodingKind::Handshake, "comline@1", "comline@1"),
            (EncodingKind::Handshake, "none", "none"),
        ];
        for (kind, written, canonical) in cases {
            assert_eq!(resolve(kind, written), Some(canonical), "{written}");
        }
        for (kind, written) in [
            (EncodingKind::Format, "packed"),
            (EncodingKind::Format, "datagram@2"),
            (EncodingKind::Framing, "packed@1"),
            (EncodingKind::Framing, "datagram@3"),
            (EncodingKind::Handshake, "comline"),
            (EncodingKind::Handshake, "None"),
        ] {
            assert_eq!(resolve(kind, written), None, "{written}");
        }
        assert_eq!(names(EncodingKind::Framing), ["jsonrpc", "datagram", "datagram@2"]);
        assert_eq!(names(EncodingKind::Handshake), ["comline@1", "none"]);
    }

    #[test]
    fn the_official_profiles_are_the_runtimes() {
        // Each profile names the encoding that declares it, which freezing
        // checks; and the hash is of the embedded bytes.
        for unit in units() {
            let FrozenUnit::Encoding { name, profile_hash, .. } = unit else { unreachable!() };
            let (_, contents) = comline_core_stdlib::OFFICIAL_PROFILES
                .iter()
                .find(|(path, _)| *path == format!("profiles/{name}.json"))
                .unwrap();
            assert_eq!(*profile_hash, blake3::hash(contents.as_bytes()).to_hex().to_string());
        }
    }
}
