//! `@unverified`: required where a protocol gives up a guarantee, gated off
//! by default, frozen with the set core works out, and compared by the diff.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::{
    analyze_schema_changes, BreakingChange, Modification, NewFeature,
};
use comline_core::schema::ir::frozen::unit::{FrozenUnit, Guarantee};
use comline_core::schema::ir::unverified;

const ALLOW: &str = "settings = { annotations = { unverified = { allowed = true } } }\n";

fn config(extra: &str) -> String {
    format!(
        "congregation app\nspecification_version = 1\n\
         code_generation = {{ languages = {{ rust#1.70.0 = {{}} }} }}\n{extra}"
    )
}

/// Compile one schema; the frozen units and the warning codes.
fn compile(config: &str, schema: &str) -> eyre::Result<(Vec<FrozenUnit>, Vec<String>)> {
    let ctx = PackageSources::new().config(config).schema(["app"], schema).compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    let warnings = schema.warnings.borrow().iter().map(|w| w.code.id.to_string()).collect();
    Ok((units, warnings))
}

fn protocol_params<'a>(units: &'a [FrozenUnit], protocol: &str) -> &'a [FrozenUnit] {
    units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Protocol { name, parameters, .. } if name == protocol => {
                Some(parameters.as_slice())
            }
            _ => None,
        })
        .unwrap()
}

const NONE_MARKED: &str =
    "@handshake = \"none\"\n@unverified = \"the gateway predates Comline\"\nprotocol P {\n    function f();\n}\n";

#[test]
fn giving_something_up_without_the_marker_is_an_error() {
    let schema = "@handshake = \"none\"\nprotocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(&config(ALLOW), schema).unwrap_err());
    assert!(err.contains("unverified::missing-marker"), "{err}");
    assert!(err.contains("schema agreement and encoding agreement"), "{err}");
}

#[test]
fn the_marker_is_off_by_default() {
    let err = format!("{:?}", compile(&config(""), NONE_MARKED).unwrap_err());
    assert!(err.contains("annotations.unverified.allowed"), "{err}");
}

#[test]
fn a_marked_protocol_freezes_what_it_gives_up() {
    let (units, warnings) = compile(&config(ALLOW), NONE_MARKED).unwrap();
    let params = protocol_params(&units, "P");
    assert_eq!(
        unverified::frozen_set(params),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert_eq!(unverified::reason(params), Some("the gateway predates Comline"));
    assert!(warnings.contains(&"unverified::schema-agreement".to_string()), "{warnings:?}");
    assert!(warnings.contains(&"unverified::encoding-agreement".to_string()), "{warnings:?}");
}

#[test]
fn the_package_default_handshake_counts_too() {
    let config = config(&format!("handshake = \"none\"\n{ALLOW}"));
    let bare = "protocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(&config, bare).unwrap_err());
    assert!(err.contains("unverified::missing-marker"), "{err}");

    let marked = "@unverified = \"legacy\"\nprotocol P {\n    function f();\n}\n";
    let (units, _) = compile(&config, marked).unwrap();
    assert_eq!(unverified::frozen_set(protocol_params(&units, "P")).len(), 2);
}

#[test]
fn a_reason_is_required() {
    let schema = "@handshake = \"none\"\n@unverified = \"  \"\nprotocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(&config(ALLOW), schema).unwrap_err());
    assert!(err.contains("unverified::missing-reason"), "{err}");
}

#[test]
fn a_marker_where_nothing_is_given_up_is_a_warning() {
    let schema = "@unverified = \"just in case\"\nprotocol P {\n    function f();\n}\n";
    let (units, warnings) = compile(&config(ALLOW), schema).unwrap();
    assert_eq!(warnings, ["unverified::nothing-given-up"]);
    assert!(unverified::frozen_set(protocol_params(&units, "P")).is_empty());
}

/// The diff reads the frozen set, which a build writes; do the same here.
fn frozen(source: &str) -> Vec<FrozenUnit> {
    let mut units = IncrementalInterpreter::from_source(source);
    unverified::freeze(&mut units);
    units
}

#[test]
fn giving_up_is_major_restoring_is_minor_rewording_is_a_patch() {
    let plain = "protocol P {\n    function f();\n}\n";
    let reworded = NONE_MARKED.replace("predates Comline", "is from 2009");

    let gave_up = analyze_schema_changes(&frozen(plain), &frozen(NONE_MARKED));
    assert!(gave_up.breaking_changes.iter().any(|c| matches!(
        c,
        BreakingChange::GaveUpGuarantees { guarantees, .. } if guarantees.len() == 2
    )));

    let restored = analyze_schema_changes(&frozen(NONE_MARKED), &frozen(plain));
    assert!(restored.new_features.iter().any(|c| matches!(
        c,
        NewFeature::RestoredGuarantees { guarantees, .. } if guarantees.len() == 2
    )));

    let changes = analyze_schema_changes(&frozen(NONE_MARKED), &frozen(&reworded));
    assert!(changes.breaking_changes.is_empty(), "{:?}", changes.breaking_changes);
    assert!(matches!(
        changes.modifications.as_slice(),
        [Modification::ChangedAnnotation { key, .. }] if key == "unverified"
    ));
}
