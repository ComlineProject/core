//! The package's `wire = { format, framing }`: the framing a protocol without
//! `@framing` gets (else `datagram@2`), frozen into each protocol it reaches,
//! and the format its generated code speaks (else `packed@1`).

use comline_core::package::build::PackageSources;
use comline_core::package::config::ir::frozen::{self as config_ir, FrozenUnit as ConfigUnit};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

const SCHEMA: &str = "protocol Plain {\n    function f();\n}\n\n\
                      struct S {\n    a: u32\n}\n";

fn config(wire: Option<&str>, extra: &str) -> String {
    let line = wire.map(|w| format!("wire = {w}\n")).unwrap_or_default();
    format!(
        "congregation app\nspecification_version = 1\n{line}\n\
         code_generation = {{\n    languages = {{\n        rust#1.70.0 = {{}}\n    }}\n}}\n{extra}"
    )
}

type Compiled = (Vec<FrozenUnit>, Vec<ConfigUnit>);

fn compile_with(wire: Option<&str>, extra: &str, schema: &str) -> eyre::Result<Compiled> {
    let ctx = PackageSources::new().config(config(wire, extra)).schema(["app"], schema).compile()?;
    let config = ctx.config_frozen.clone().unwrap();
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    Ok((units, config))
}

fn compile(wire: Option<&str>, schema: &str) -> eyre::Result<Compiled> {
    compile_with(wire, "", schema)
}

/// A protocol's `@framing`, as frozen.
fn framing_of(units: &[FrozenUnit], protocol: &str) -> Option<String> {
    units.iter().find_map(|u| match u {
        FrozenUnit::Protocol { name, parameters, .. } if name == protocol => {
            Some(parameters.iter().find_map(|p| match p {
                FrozenUnit::Property { name, value: ConfigValue::Str(v) } if name == "framing" => Some(v.clone()),
                _ => None,
            }))
        }
        _ => None,
    })?
}

#[test]
fn nothing_declared_means_packed_on_datagram2_left_implicit() {
    let (units, config) = compile(None, SCHEMA).unwrap();
    assert_eq!(framing_of(&units, "Plain"), None);
    assert_eq!(config_ir::wire_format(&config), "packed@1");
    assert_eq!(config_ir::wire_framing(&config), "datagram@2");

    // Writing the defaults out changes no schema IR.
    let (explicit, config) =
        compile(Some("{ format = \"packed@1\" framing = \"datagram@2\" }"), SCHEMA).unwrap();
    assert_eq!(explicit, units);
    assert_eq!(config_ir::wire(&config), (Some("packed@1"), Some("datagram@2")));
}

#[test]
fn the_package_framing_is_frozen_into_protocols_without_their_own() {
    let schema = format!("{SCHEMA}\n@framing = \"datagram@2\"\nprotocol Own {{\n    function g();\n}}\n");
    let allow = "settings = { annotations = { framing = { allowed = true } } }\n";
    let (units, _) = compile_with(Some("{ framing = \"datagram@1\" }"), allow, &schema).unwrap();
    // Canonical: `datagram@1` is a spelling of `datagram`.
    assert_eq!(framing_of(&units, "Plain").as_deref(), Some("datagram"));
    assert_eq!(framing_of(&units, "Own").as_deref(), Some("datagram@2"));
    let s = units.iter().find(|u| matches!(u, FrozenUnit::Struct { name, .. } if name == "S"));
    assert!(matches!(s, Some(FrozenUnit::Struct { parameters, .. }) if parameters.is_empty()));
}

#[test]
fn the_package_framing_isnt_held_to_the_framing_gate() {
    // `@framing` is forbidden by default; the package's own choice isn't
    // an annotation in a schema, so it goes through.
    let (units, _) = compile(Some("{ framing = \"jsonrpc\" }"), SCHEMA).unwrap();
    assert_eq!(framing_of(&units, "Plain").as_deref(), Some("jsonrpc"));
}

#[test]
fn a_declared_format_other_than_json_is_refused_next_to_jsonrpc() {
    let err = compile(Some("{ format = \"packed@1\" framing = \"jsonrpc\" }"), SCHEMA)
        .unwrap_err()
        .to_string();
    assert!(err.contains("protocol `Plain` uses JSON-RPC"), "{err}");
    assert!(err.contains("`wire.format = \"packed@1\"`"), "{err}");

    compile(Some("{ format = \"json@1\" framing = \"jsonrpc\" }"), SCHEMA).unwrap();
    // Left out, the format applies to the other protocols only.
    compile(Some("{ framing = \"jsonrpc\" }"), SCHEMA).unwrap();
}

#[test]
fn wire_entries_are_checked() {
    for (wire, expected) in [
        ("{ format = \"packed\" }", "wire format `packed` isn't known"),
        ("{ framing = \"datagram@3\" }", "framing `datagram@3` isn't known"),
        ("{ codec = \"json@1\" }", "'codec' is not allowed in 'wire'"),
        ("{ format = \"json@1\" format = \"packed@1\" }", "'wire.format' is set twice"),
        ("\"packed@1\"", "'wire' should be a dictionary"),
    ] {
        let err = format!("{:?}", compile(Some(wire), SCHEMA).unwrap_err());
        assert!(err.contains(expected), "{wire}: {err}");
    }
}
