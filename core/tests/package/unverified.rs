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
    unverified::freeze(&mut units, &Default::default());
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

#[test]
fn comline_and_none_give_up_what_they_always_did() {
    let encodings = unverified::Encodings::default();
    let params = |handshake: &str| {
        vec![FrozenUnit::Property {
            name: "handshake".into(),
            value: comline_core::schema::ir::frozen::unit::ConfigValue::Str(handshake.into()),
        }]
    };
    assert!(unverified::given_up(&[], &encodings).is_empty());
    assert!(unverified::given_up(&params("comline@1"), &encodings).is_empty());
    assert_eq!(unverified::cause(&params("comline@1"), &encodings), "handshake `comline@1`");
    assert_eq!(
        unverified::given_up(&params("none"), &encodings),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert_eq!(unverified::cause(&params("none"), &encodings), "handshake `none`");
}

// Bounded size across schemas: a type another schema declares may hold a
// value without a maximum that only that schema's marked protocols reach.
// Wherever it's used, it's held to the same rule.

const MODELS: &str = "struct Blob {\n    data: string\n}\n\n\
     @unverified = \"the uploader streams whatever it's given\"\n\
     protocol Upload {\n    function put(b: Blob);\n}\n";

/// `api`, then the schemas it uses: `api` is added first, so it builds
/// only once they have.
fn compile_with(schemas: &[(&str, &str)], api: &str) -> eyre::Result<Vec<FrozenUnit>> {
    let mut sources = PackageSources::new()
        .config(&config(ALLOW))
        .schema(["api"], api);
    for (name, schema) in schemas {
        sources = sources.schema([*name], *schema);
    }
    let ctx = sources.compile()?;
    let api = ctx.schema_contexts[0].borrow();
    let units = api.frozen_schema.borrow().clone().unwrap();
    Ok(units)
}

const ORDERS: &str = "protocol Orders {\n    function send(b: Blob);\n}\n";
const MARKED_ORDERS: &str =
    "@unverified = \"orders carry the upload\"\nprotocol Orders {\n    function send(b: Blob);\n}\n";

#[test]
fn an_unbounded_type_from_another_schema_is_unbounded_where_used() {
    let err = compile_with(
        &[("models", MODELS)],
        &format!("use models::Blob\n\n{ORDERS}"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("protocol `Orders`, function `send`, argument `b`: `Blob` holds a value without a maximum"),
        "{err}"
    );
}

#[test]
fn a_marked_protocol_using_it_gives_up_bounded_size() {
    let units = compile_with(
        &[("models", MODELS)],
        &format!("use models::Blob\n\n{MARKED_ORDERS}"),
    )
    .unwrap();
    assert_eq!(
        unverified::frozen_set(protocol_params(&units, "Orders")),
        [Guarantee::BoundedSize]
    );
}

#[test]
fn a_type_bounded_where_declared_is_bounded_where_used() {
    let models = MODELS.replace("data: string", "data: char[max 64]");
    let units = compile_with(
        &[("models", &models)],
        &format!("use models::Blob\n\n{ORDERS}"),
    )
    .unwrap();
    assert!(unverified::frozen_set(protocol_params(&units, "Orders")).is_empty());
}

#[test]
fn it_stays_unbounded_through_a_rename_a_struct_and_another_schema() {
    let renamed = "use models::Blob as Payload\n\n\
         protocol Orders {\n    function send(b: Payload);\n}\n";
    let err = compile_with(&[("models", MODELS)], renamed)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("argument `b`: `Payload` holds a value without a maximum"),
        "{err}"
    );

    // `Order` holds a `Blob`; no protocol here reaches it, so another
    // schema could use it, and it has to be bounded.
    let held = "use models::Blob\n\nstruct Order {\n    blob: Blob\n}\n";
    let err = compile_with(&[("models", MODELS)], held)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("struct `Order`, field `blob`: `Blob` holds a value without a maximum"),
        "{err}"
    );

    // `wrap` holds it in `Wrapper`, reached only by its own marked
    // protocol; `api` uses `Wrapper`.
    let wrap = "use models::Blob\n\nstruct Wrapper {\n    blob: Blob\n}\n\n\
         @unverified = \"wraps uploads\"\nprotocol Wrap {\n    function wrap(w: Wrapper);\n}\n";
    let api = "use wrap::Wrapper\n\nprotocol Orders {\n    function send(w: Wrapper);\n}\n";
    let err = compile_with(&[("wrap", wrap), ("models", MODELS)], api)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("argument `w`: `Wrapper` holds a value without a maximum"),
        "{err}"
    );
}
