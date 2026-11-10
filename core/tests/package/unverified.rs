//! `@unverified`: required where a protocol gives up a guarantee, gated off
//! by default, frozen with the set core works out, and compared by the diff.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::{
    analyze_schema_changes, BreakingChange, Modification, NewFeature,
};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
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

/// `NONE_MARKED`, with a reason for each guarantee a `none` handshake gives up.
const NONE_KEYED: &str = "@handshake = \"none\"\n\
     @unverified = {\n    schema_agreement = \"the gateway predates Comline\",\n    \
     encoding_agreement = \"the gateway only speaks one encoding\"\n}\n\
     protocol P {\n    function f();\n}\n";

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

    // The keyed form: rewording a reason, or switching between the forms,
    // changes no set, so it's a patch as well.
    let keyed_reworded = NONE_KEYED.replace("the gateway", "the old gateway");
    for (old, new) in [(NONE_KEYED, keyed_reworded.as_str()), (NONE_MARKED, NONE_KEYED), (NONE_KEYED, NONE_MARKED)] {
        let changes = analyze_schema_changes(&frozen(old), &frozen(new));
        assert!(changes.breaking_changes.is_empty(), "{:?}", changes.breaking_changes);
        assert!(changes.new_features.is_empty(), "{:?}", changes.new_features);
        assert!(matches!(
            changes.modifications.as_slice(),
            [Modification::ChangedAnnotation { key, .. }] if key == "unverified"
        ));
    }

    let gave_up = analyze_schema_changes(&frozen(plain), &frozen(NONE_KEYED));
    assert!(gave_up.breaking_changes.iter().any(|c| matches!(
        c,
        BreakingChange::GaveUpGuarantees { guarantees, .. } if guarantees.len() == 2
    )));
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

// The keyed form, `@unverified = { <guarantee> = "<reason>" }`: a reason for
// each guarantee given up, under its id in the specification's catalog.

#[test]
fn the_keyed_form_covering_exactly_the_set_is_accepted() {
    let (units, warnings) = compile(&config(ALLOW), NONE_KEYED).unwrap();
    let params = protocol_params(&units, "P");
    // The frozen set is what core works out; the reasons stay in the annotation.
    assert_eq!(
        unverified::frozen_set(params),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert_eq!(
        unverified::marker(params),
        Some(&ConfigValue::Dict(vec![
            ("schema_agreement".into(), ConfigValue::Str("the gateway predates Comline".into())),
            (
                "encoding_agreement".into(),
                ConfigValue::Str("the gateway only speaks one encoding".into())
            ),
        ]))
    );
    assert_eq!(unverified::reason(params), None);
    assert_eq!(
        unverified::reason_for(params, Guarantee::EncodingAgreement),
        Some("the gateway only speaks one encoding")
    );
    assert_eq!(unverified::reason_for(params, Guarantee::BoundedSize), None);
    assert_eq!(warnings, ["unverified::schema-agreement", "unverified::encoding-agreement"]);
}

#[test]
fn the_keyed_form_is_shown_with_each_guarantees_own_reason() {
    let (plain, _) = compile(&config(ALLOW), NONE_MARKED).unwrap();
    let (keyed, _) = compile(&config(ALLOW), NONE_KEYED).unwrap();
    let set = [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement];
    assert_eq!(
        unverified::protocols(&plain),
        [("P", &set[..], Some("\"the gateway predates Comline\"".to_string()))]
    );
    assert_eq!(
        unverified::protocols(&keyed),
        [(
            "P",
            &set[..],
            Some(
                "schema agreement \"the gateway predates Comline\", \
                 encoding agreement \"the gateway only speaks one encoding\""
                    .to_string()
            )
        )]
    );
}

#[test]
fn an_unknown_key_is_an_error_with_a_suggestion() {
    let schema = NONE_KEYED.replace("\n}\nprotocol", ",\n    bounded_sise = \"x\"\n}\nprotocol");
    let err = format!("{:?}", compile(&config(ALLOW), &schema).unwrap_err());
    assert!(err.contains("unverified::unknown-guarantee"), "{err}");
    assert!(err.contains("`bounded_sise` isn't a guarantee - did you mean `bounded_size`?"), "{err}");
    assert!(!err.contains("missing-guarantee"), "{err}");

    let schema = NONE_KEYED.replace("encoding_agreement", "wibble");
    let err = format!("{:?}", compile(&config(ALLOW), &schema).unwrap_err());
    assert!(err.contains("`wibble` isn't a guarantee") && !err.contains("did you mean"), "{err}");
}

#[test]
fn a_guarantee_given_up_and_not_listed_is_an_error() {
    let schema = NONE_KEYED.replace(",\n    encoding_agreement = \"the gateway only speaks one encoding\"", "");
    let err = format!("{:?}", compile(&config(ALLOW), &schema).unwrap_err());
    assert!(err.contains("unverified::missing-guarantee"), "{err}");
    assert!(err.contains("gives up encoding agreement (handshake `none`)"), "{err}");
    assert!(err.contains("add `encoding_agreement = \"...\"` to `@unverified`"), "{err}");
    assert!(!err.contains("schema agreement (handshake"), "{err}");
}

#[test]
fn a_guarantee_listed_and_not_given_up_is_a_warning() {
    let schema = NONE_KEYED.replace("\n}\nprotocol", ",\n    version_honesty = \"just in case\"\n}\nprotocol");
    let (units, warnings) = compile(&config(ALLOW), &schema).unwrap();
    assert!(warnings.contains(&"unverified::not-given-up".to_string()), "{warnings:?}");
    assert_eq!(
        unverified::frozen_set(protocol_params(&units, "P")),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
}

#[test]
fn a_keyed_reason_has_to_be_a_string_that_says_something() {
    for reason in ["\"  \"", "\"\"", "true"] {
        let schema = NONE_KEYED.replace("\"the gateway predates Comline\"", reason);
        let err = format!("{:?}", compile(&config(ALLOW), &schema).unwrap_err());
        assert!(err.contains("unverified::missing-reason"), "{reason}: {err}");
        assert!(err.contains("needs a reason for `schema_agreement`"), "{reason}: {err}");
    }
}

#[test]
fn a_guarantee_listed_twice_is_an_error() {
    let schema = NONE_KEYED.replace("\n}\nprotocol", ",\n    schema_agreement = \"again\"\n}\nprotocol");
    let err = format!("{:?}", compile(&config(ALLOW), &schema).unwrap_err());
    assert!(err.contains("unverified::duplicate-guarantee"), "{err}");
}

#[test]
fn bounded_size_has_to_be_listed_where_a_string_has_no_maximum() {
    let unbounded = |marker: &str| {
        format!("{marker}protocol P {{\n    function put(s: string);\n}}\n")
    };
    let keyed = unbounded("@unverified = { bounded_size = \"the uploader streams whatever it's given\" }\n");
    let (units, warnings) = compile(&config(ALLOW), &keyed).unwrap();
    assert_eq!(unverified::frozen_set(protocol_params(&units, "P")), [Guarantee::BoundedSize]);
    assert!(warnings.contains(&"unverified::bounded-size".to_string()), "{warnings:?}");

    // Listing something else doesn't cover it.
    let other = unbounded("@unverified = { version_honesty = \"not this\" }\n");
    let err = format!("{:?}", compile(&config(ALLOW), &other).unwrap_err());
    assert!(err.contains("unverified::missing-guarantee"), "{err}");
    assert!(err.contains("bounded size") && err.contains("add `bounded_size = \"...\"`"), "{err}");
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

// The package's own `unverified`, in `config.idp`: why the package gives up
// what its own settings give up, once for every protocol that takes them.

const WHY: &str = "the gateway pins schemas by contract";

/// A package with no handshake, its `unverified` as given.
fn without_handshake(unverified: &str) -> String {
    config(&format!("handshake = \"none\"\n{unverified}{ALLOW}"))
}

/// `unverified`, keyed, with `WHY` for what a `none` handshake gives up.
const PACKAGE_KEYED: &str =
    "unverified = {\n    schema_agreement = \"the gateway pins schemas by contract\"\n    \
     encoding_agreement = \"the gateway pins schemas by contract\"\n}\n";

const BARE: &str = "protocol P {\n    function f();\n}\n";

fn package_reasons() -> ConfigValue {
    ConfigValue::Dict(vec![
        ("schema_agreement".into(), ConfigValue::Str(WHY.into())),
        ("encoding_agreement".into(), ConfigValue::Str(WHY.into())),
    ])
}

#[test]
fn a_package_acknowledgement_covers_a_protocol_that_takes_the_package_handshake() {
    let (units, warnings) = compile(&without_handshake(PACKAGE_KEYED), BARE).unwrap();
    let params = protocol_params(&units, "P");
    // What's given up is what core works out, marker or not.
    assert_eq!(
        unverified::frozen_set(params),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert_eq!(unverified::marker(params), None);
    assert_eq!(unverified::package_scope(params), Some(&package_reasons()));
    assert_eq!(unverified::reason_for(params, Guarantee::SchemaAgreement), Some(WHY));
    assert_eq!(warnings, ["unverified::schema-agreement", "unverified::encoding-agreement"]);

    // Consumers are shown the package's reasons.
    let set = [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement];
    assert_eq!(
        unverified::protocols(&units),
        [(
            "P",
            &set[..],
            Some(format!("schema agreement \"{WHY}\", encoding agreement \"{WHY}\""))
        )]
    );
}

#[test]
fn the_plain_form_covers_every_guarantee_a_package_setting_gives_up() {
    let config = without_handshake(&format!("unverified = \"{WHY}\"\n"));
    let (units, _) = compile(&config, BARE).unwrap();
    let params = protocol_params(&units, "P");
    assert_eq!(unverified::frozen_set(params).len(), 2);
    // Frozen as the dictionary, the plain reason for each guarantee covered.
    assert_eq!(unverified::package_scope(params), Some(&package_reasons()));
}

#[test]
fn a_protocol_with_its_own_handshake_is_not_covered() {
    let own = "@handshake = \"none\"\nprotocol Q {\n    function g();\n}\n";
    let schema = format!("{BARE}\n{own}");
    let err = format!("{:?}", compile(&without_handshake(PACKAGE_KEYED), &schema).unwrap_err());
    assert!(err.contains("unverified::missing-marker"), "{err}");
    assert!(err.contains("protocol `Q` gives up schema agreement and encoding agreement"), "{err}");
    assert!(!err.contains("protocol `P` gives up"), "{err}");

    // A protocol can't write the property the compiler freezes the package's
    // reasons in: only the compiler says what the package covers.
    let forged = "@handshake = \"none\"\n\
         @unverified_package = {\n    schema_agreement = \"mine\",\n    encoding_agreement = \"mine\"\n}\n\
         protocol Q {\n    function g();\n}\n";
    let err = format!("{:?}", compile(&without_handshake(PACKAGE_KEYED), forged).unwrap_err());
    assert!(err.contains("unverified::missing-marker"), "{err}");
}

#[test]
fn bounded_size_is_not_the_packages_to_acknowledge() {
    // The handshake's losses are covered; the unbounded string isn't.
    let unbounded = "protocol P {\n    function put(s: string);\n}\n";
    let err = format!("{:?}", compile(&without_handshake(PACKAGE_KEYED), unbounded).unwrap_err());
    assert!(err.contains("argument `s`: `string` has no maximum"), "{err}");

    let keyed = format!("unverified = {{ bounded_size = \"{WHY}\" }}\n");
    let err = format!("{:?}", compile(&without_handshake(&keyed), BARE).unwrap_err());
    assert!(err.contains("unverified::wrong-scope"), "{err}");
    assert!(
        err.contains("`bounded_size` can't be acknowledged at package scope; the scopes it allows are protocol, function, argument, field, type"),
        "{err}"
    );
}

#[test]
fn the_package_acknowledgement_is_checked_like_a_marker() {
    let err = format!("{:?}", compile(&without_handshake("unverified = { schema_agreemnt = \"x\" }\n"), BARE).unwrap_err());
    assert!(err.contains("unverified::unknown-guarantee"), "{err}");
    assert!(err.contains("`schema_agreemnt` isn't a guarantee - did you mean `schema_agreement`?"), "{err}");

    for reason in ["\"\"", "\"  \"", "true"] {
        let keyed = format!("unverified = {{ schema_agreement = {reason} }}\n");
        let err = format!("{:?}", compile(&without_handshake(&keyed), BARE).unwrap_err());
        assert!(err.contains("unverified::missing-reason"), "{reason}: {err}");
        assert!(err.contains("needs a reason for `schema_agreement`"), "{reason}: {err}");
    }
    let err = format!("{:?}", compile(&without_handshake("unverified = \"\"\n"), BARE).unwrap_err());
    assert!(err.contains("unverified::missing-reason"), "{err}");

    let twice = "unverified = { schema_agreement = \"a\" schema_agreement = \"b\" }\n";
    let err = format!("{:?}", compile(&without_handshake(twice), BARE).unwrap_err());
    assert!(err.contains("unverified::duplicate-guarantee"), "{err}");
}

#[test]
fn a_guarantee_no_protocol_gives_up_through_the_package_is_a_warning() {
    let keyed = format!(
        "unverified = {{\n    schema_agreement = \"{WHY}\"\n    encoding_agreement = \"{WHY}\"\n    \
         version_honesty = \"just in case\"\n}}\n"
    );
    let (units, warnings) = compile(&without_handshake(&keyed), BARE).unwrap();
    assert!(warnings.contains(&"unverified::not-given-up".to_string()), "{warnings:?}");
    // It isn't in what the protocol is covered for.
    assert_eq!(unverified::package_reason_for(protocol_params(&units, "P"), Guarantee::VersionHonesty), None);

    // The handshake is the default one: there's nothing for it to cover.
    let (units, warnings) = compile(&config(&format!("{PACKAGE_KEYED}{ALLOW}")), BARE).unwrap();
    assert_eq!(warnings.iter().filter(|w| *w == "unverified::not-given-up").count(), 2, "{warnings:?}");
    assert_eq!(unverified::package_scope(protocol_params(&units, "P")), None);
    assert!(unverified::frozen_set(protocol_params(&units, "P")).is_empty());
}

#[test]
fn a_keyed_marker_need_not_list_what_the_package_covers() {
    let unbounded = |marker: &str| format!("{marker}protocol P {{\n    function put(s: string);\n}}\n");
    let only_bounded = unbounded("@unverified = { bounded_size = \"the uploader streams whatever it's given\" }\n");
    let (units, _) = compile(&without_handshake(PACKAGE_KEYED), &only_bounded).unwrap();
    let params = protocol_params(&units, "P");
    assert_eq!(
        unverified::frozen_set(params),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement, Guarantee::BoundedSize]
    );
    // Its own reason for one, the package's for the rest.
    assert_eq!(
        unverified::reason_for(params, Guarantee::BoundedSize),
        Some("the uploader streams whatever it's given")
    );
    assert_eq!(unverified::reason_for(params, Guarantee::EncodingAgreement), Some(WHY));

    // Listing one the package covers is fine too, and its own reason wins.
    let both = unbounded(
        "@unverified = {\n    schema_agreement = \"mine\",\n    bounded_size = \"the uploader streams whatever it's given\"\n}\n",
    );
    let (units, warnings) = compile(&without_handshake(PACKAGE_KEYED), &both).unwrap();
    assert!(!warnings.contains(&"unverified::not-given-up".to_string()), "{warnings:?}");
    assert_eq!(unverified::reason_for(protocol_params(&units, "P"), Guarantee::SchemaAgreement), Some("mine"));

    // Without the package, the same marker leaves them unsaid.
    let err = format!("{:?}", compile(&config(&format!("handshake = \"none\"\n{ALLOW}")), &only_bounded).unwrap_err());
    assert!(err.contains("unverified::missing-guarantee"), "{err}");
}

#[test]
fn a_package_acknowledgement_is_off_without_the_gate() {
    let config = config(&format!("handshake = \"none\"\n{PACKAGE_KEYED}"));
    let err = format!("{:?}", compile(&config, BARE).unwrap_err());
    assert!(err.contains("annotations.unverified.allowed"), "{err}");
    assert!(err.contains("config.idp's `unverified`"), "{err}");
}

#[test]
fn rewording_the_package_reason_is_a_patch_and_moving_it_is_too() {
    let compiled = |config: &str, schema: &str| compile(config, schema).unwrap().0;
    let old = compiled(&without_handshake(PACKAGE_KEYED), BARE);
    let reworded = compiled(&without_handshake(&PACKAGE_KEYED.replace("pins schemas", "fixes the schemas")), BARE);
    let changes = analyze_schema_changes(&old, &reworded);
    assert!(changes.breaking_changes.is_empty(), "{:?}", changes.breaking_changes);
    assert!(changes.new_features.is_empty(), "{:?}", changes.new_features);
    assert!(matches!(
        changes.modifications.as_slice(),
        [Modification::ChangedAnnotation { key, .. }] if key == "unverified_package"
    ));

    // The protocol says it itself, as it could before: the same set is given
    // up, so this changes what's said and no more.
    let own = compiled(&without_handshake(""), NONE_MARKED.replace("@handshake = \"none\"\n", "").as_str());
    let changes = analyze_schema_changes(&old, &own);
    assert!(changes.breaking_changes.is_empty(), "{:?}", changes.breaking_changes);
    assert!(changes.new_features.is_empty(), "{:?}", changes.new_features);
    assert!(matches!(
        changes.modifications.as_slice(),
        [Modification::ChangedAnnotation { key, .. }] if key == "unverified_package"
    ));

    // What's given up decides major and minor, not who says why.
    let plain = compiled(&config(ALLOW), BARE);
    let changes = analyze_schema_changes(&plain, &old);
    assert!(changes.breaking_changes.iter().any(|c| matches!(
        c,
        BreakingChange::GaveUpGuarantees { guarantees, .. } if guarantees.len() == 2
    )));
    let changes = analyze_schema_changes(&old, &plain);
    assert!(changes.new_features.iter().any(|c| matches!(
        c,
        NewFeature::RestoredGuarantees { guarantees, .. } if guarantees.len() == 2
    )));
}
