//! `schema::ir::package_wire::apply_package_wire`, the public form of what
//! the compiler does with a package's framing and handshake, gives the same
//! IR as compiling the package, for every way a package can arrive at them.

use comline_core::package::build::PackageSources;
use comline_core::schema::idl::parse::parse_schema;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use comline_core::schema::ir::package_wire::apply_package_wire;

/// A package with `config_lines`, and `schemas` as `(namespace, source)`.
/// Returns, per schema, the compiler's IR and the IR the schema interpreted
/// on its own gets from `apply_package_wire`.
fn both(config_lines: &str, schemas: &[(&str, &str)]) -> Vec<(Vec<FrozenUnit>, Vec<FrozenUnit>)> {
    let config = format!(
        "congregation app\nspecification_version = 1\n{config_lines}\n\
         settings = {{ annotations = {{ framing = {{ allowed = true }} }} }}\n"
    );
    let mut sources = PackageSources::new().config(config);
    for (namespace, source) in schemas {
        sources = sources.schema(["app", namespace], *source);
    }
    let ctx = sources.compile().unwrap();
    let config = ctx.config_frozen.clone().unwrap();

    let compiled: Vec<Vec<FrozenUnit>> = schemas
        .iter()
        .map(|(namespace, _)| {
            let joined = format!("app::{namespace}");
            let schema = ctx
                .schema_contexts
                .iter()
                .find(|s| s.borrow().namespace_joined() == joined)
                .unwrap()
                .borrow();
            let units = schema.frozen_schema.borrow().clone().unwrap();
            units
        })
        .collect();

    let mut interpreted: Vec<Vec<FrozenUnit>> = schemas
        .iter()
        .map(|(namespace, source)| {
            let document = parse_schema(source).unwrap();
            let mut units = IncrementalInterpreter::from_declarations(document.0);
            units.insert(0, FrozenUnit::Namespace(format!("app::{namespace}")));
            units
        })
        .collect();
    let mut slices: Vec<&mut [FrozenUnit]> = interpreted.iter_mut().map(Vec::as_mut_slice).collect();
    apply_package_wire(&mut slices, &config);

    compiled.into_iter().zip(interpreted).collect()
}

/// A protocol's frozen `key` parameter.
fn property(units: &[FrozenUnit], protocol: &str, key: &str) -> Option<ConfigValue> {
    units.iter().find_map(|u| match u {
        FrozenUnit::Protocol { name, parameters, .. } if name == protocol => parameters.iter().find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == key => Some(value.clone()),
            _ => None,
        }),
        _ => None,
    })
}

fn str(value: &str) -> Option<ConfigValue> {
    Some(ConfigValue::Str(value.to_string()))
}

fn assert_same(pairs: &[(Vec<FrozenUnit>, Vec<FrozenUnit>)]) {
    for (compiled, applied) in pairs {
        assert_eq!(compiled, applied);
    }
}

const PLAIN: &str = "protocol Plain {\n    function f();\n}\n";
const OTHER: &str = "protocol Other {\n    function g();\n}\n";
const JSONRPC: &str = "@framing = \"jsonrpc\"\nprotocol Rpc {\n    function h();\n}\n";

#[test]
fn defaults_freeze_datagram_3_and_comline_2() {
    let pairs = both("", &[("a", PLAIN), ("b", OTHER)]);
    assert_same(&pairs);
    let (_, a) = &pairs[0];
    assert_eq!(property(a, "Plain", "framing"), str("datagram@3"));
    assert_eq!(property(a, "Plain", "handshake"), str("comline@2"));
}

#[test]
fn one_jsonrpc_protocol_keeps_the_whole_package_on_comline_1() {
    // The default handshake is one for the package, across its schemas: a
    // protocol on `jsonrpc` keeps every protocol on `comline@1`, implicit.
    let pairs = both("", &[("a", PLAIN), ("b", JSONRPC)]);
    assert_same(&pairs);
    let (a, b) = (&pairs[0].1, &pairs[1].1);
    assert_eq!(property(a, "Plain", "framing"), str("datagram@3"));
    assert_eq!(property(a, "Plain", "handshake"), None);
    assert_eq!(property(b, "Rpc", "framing"), str("jsonrpc"));
    assert_eq!(property(b, "Rpc", "handshake"), None);
}

#[test]
fn a_protocol_naming_comline_2_keeps_the_default() {
    let own = "@handshake = \"comline@2\"\nprotocol Own {\n    function g();\n}\n";
    let pairs = both("", &[("a", PLAIN), ("b", own)]);
    assert_same(&pairs);
    assert_eq!(property(&pairs[0].1, "Plain", "handshake"), str("comline@2"));
}

#[test]
fn a_stated_framing_and_handshake_freeze_as_written() {
    // `datagram@2` stays implicit and its default handshake is `comline@1`.
    let pairs = both("wire = { framing = \"datagram@2\" }", &[("a", PLAIN), ("b", JSONRPC)]);
    assert_same(&pairs);
    assert_eq!(property(&pairs[0].1, "Plain", "framing"), None);
    assert_eq!(property(&pairs[0].1, "Plain", "handshake"), None);

    // A stated `comline@1` wins over the default `comline@2`, and stays
    // implicit.
    let pairs = both("handshake = \"comline@1\"", &[("a", PLAIN), ("b", OTHER)]);
    assert_same(&pairs);
    assert_eq!(property(&pairs[0].1, "Plain", "framing"), str("datagram@3"));
    assert_eq!(property(&pairs[0].1, "Plain", "handshake"), None);
}

#[test]
fn handshake_parameters_freeze_in_dictionary_form() {
    let pairs = both(
        "handshake = { name = \"comline@2\" scope = \"package\" }",
        &[("a", PLAIN), ("b", OTHER)],
    );
    assert_same(&pairs);
    assert!(matches!(property(&pairs[0].1, "Plain", "handshake"), Some(ConfigValue::Dict(_))));
}
