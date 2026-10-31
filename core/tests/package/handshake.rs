//! The handshake a protocol runs: its own `@handshake`, else the package's
//! `handshake = "..."`, else `comline@1`. The package default is frozen into
//! each protocol it reaches.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

const SCHEMA: &str = "@unverified = \"talks to a legacy peer\"\nprotocol Plain {\n    function f();\n}\n\n\
                      @handshake = \"comline@1\"\nprotocol Own {\n    function g();\n}\n\n\
                      struct S {\n    a: u32\n}\n";

fn config(handshake: Option<&str>) -> String {
    let line = handshake.map(|h| format!("handshake = \"{h}\"\n")).unwrap_or_default();
    format!(
        "congregation app\nspecification_version = 1\n{line}\n\
         code_generation = {{\n    languages = {{\n        rust#1.70.0 = {{}}\n    }}\n}}\n\
         settings = {{ annotations = {{ unverified = {{ allowed = true }} }} }}\n"
    )
}

fn compile(handshake: Option<&str>, schema: &str) -> eyre::Result<Vec<FrozenUnit>> {
    let ctx = PackageSources::new().config(config(handshake)).schema(["app"], schema).compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    Ok(units)
}

/// A protocol's `@handshake`, as frozen.
fn handshake_of(units: &[FrozenUnit], protocol: &str) -> Option<String> {
    units.iter().find_map(|u| match u {
        FrozenUnit::Protocol { name, parameters, .. } if name == protocol => {
            Some(parameters.iter().find_map(|p| match p {
                FrozenUnit::Property { name, value: ConfigValue::Str(v) } if name == "handshake" => {
                    Some(v.clone())
                }
                _ => None,
            }))
        }
        _ => None,
    })?
}

#[test]
fn the_package_default_is_frozen_into_protocols_without_their_own() {
    let units = compile(Some("none"), SCHEMA).unwrap();
    assert_eq!(handshake_of(&units, "Plain").as_deref(), Some("none"));
    // A protocol's own annotation wins.
    assert_eq!(handshake_of(&units, "Own").as_deref(), Some("comline@1"));
    // A struct takes nothing.
    let s = units.iter().find(|u| matches!(u, FrozenUnit::Struct { name, .. } if name == "S"));
    assert!(matches!(s, Some(FrozenUnit::Struct { parameters, .. }) if parameters.is_empty()));
}

#[test]
fn comline_at_1_stays_implicit_so_ir_is_unchanged() {
    let absent = compile(None, SCHEMA).unwrap();
    let explicit = compile(Some("comline@1"), SCHEMA).unwrap();
    assert_eq!(handshake_of(&absent, "Plain"), None);
    assert_eq!(absent, explicit);
}

#[test]
fn an_unknown_package_handshake_is_refused() {
    let typo = compile(Some("comline"), SCHEMA).unwrap_err().to_string();
    assert!(typo.contains("handshake `comline` isn't known"), "{typo}");

    // A namespaced one has to be declared, by the package or a dependency.
    let custom = compile(Some("com.acme.hello@1"), SCHEMA).unwrap_err().to_string();
    assert!(custom.contains("handshake `com.acme.hello@1` isn't declared"), "{custom}");
}

#[test]
fn an_unknown_handshake_annotation_is_refused() {
    let schema = "@handshake = \"com.acme.hello@1\"\nprotocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(None, schema).unwrap_err());
    assert!(err.contains("handshake `com.acme.hello@1` isn't declared"), "{err}");
}
