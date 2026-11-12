//! `@unverified` below the protocol (core#222, item 1): on a function, an
//! argument, a field or a type. The scopes a guarantee allows are the
//! catalog's; a marker acknowledges what is reached through the element it
//! sits on, and never takes a cause out of the set.

use comline_catalog::Scope;
use comline_core::inspect;
use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenArgument, FrozenUnit, Guarantee};
use comline_core::schema::ir::unverified::{self, Acknowledgement};

const ALLOW: &str = "settings = { annotations = { unverified = { allowed = true } } }\n";

fn config(extra: &str) -> String {
    format!(
        "congregation app\nspecification_version = 1\n\
         code_generation = {{ languages = {{ rust#1.70.0 = {{}} }} }}\n{extra}"
    )
}

/// What a build of `schema` gave: its frozen units and warning codes.
type Built = (Vec<FrozenUnit>, Vec<String>);

fn compile(config: &str, schema: &str) -> eyre::Result<Built> {
    let ctx = PackageSources::new()
        .config(config)
        .schema(["app"], schema)
        .compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    let warnings = schema
        .warnings
        .borrow()
        .iter()
        .map(|w| w.code.id.to_string())
        .collect();
    Ok((units, warnings))
}

fn built(schema: &str) -> Built {
    compile(&config(ALLOW), schema).unwrap_or_else(|e| panic!("{e:?}"))
}

fn error(schema: &str) -> String {
    format!("{:?}", compile(&config(ALLOW), schema).unwrap_err())
}

fn params<'a>(units: &'a [FrozenUnit], protocol: &str) -> &'a [FrozenUnit] {
    units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Protocol {
                name, parameters, ..
            } if name == protocol => Some(parameters.as_slice()),
            _ => None,
        })
        .unwrap()
}

/// Where the protocol's bounded size was acknowledged: scope, path, reason.
fn at(units: &[FrozenUnit], protocol: &str) -> Vec<(Scope, String, String)> {
    unverified::acknowledgements(params(units, protocol))
        .into_iter()
        .filter(|a| a.guarantee == Guarantee::BoundedSize)
        .map(|a| (a.scope, a.path, a.reason))
        .collect()
}

const WHY: &str = "legacy blob";

#[test]
fn the_keyed_form_is_the_dictionary_form() {
    let call =
        built("@unverified(bounded_size = \"x\")\nprotocol P {\n    function f(s: string);\n}\n").0;
    let dict = built(
        "@unverified = { bounded_size = \"x\" }\nprotocol P {\n    function f(s: string);\n}\n",
    )
    .0;
    assert_eq!(
        unverified::marker(params(&call, "P")),
        Some(&ConfigValue::Dict(vec![(
            "bounded_size".into(),
            ConfigValue::Str("x".into())
        )]))
    );
    assert_eq!(
        unverified::marker(params(&call, "P")),
        unverified::marker(params(&dict, "P"))
    );
}

#[test]
fn writing_both_forms_is_an_error() {
    let err =
        error("@unverified = \"a\"(bounded_size = \"b\")\nprotocol P {\n    function f();\n}\n");
    assert!(err.contains("annotations::both-forms"), "{err}");
}

// (a)
#[test]
fn an_argument_marker_covers_an_unbounded_argument() {
    let (units, _) = built(&format!(
        "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string);\n}}\n"
    ));
    let params = params(&units, "P");
    assert_eq!(unverified::frozen_set(params), [Guarantee::BoundedSize]);
    assert_eq!(
        at(&units, "P"),
        [(Scope::Argument, "P.f(data)".to_string(), WHY.to_string())]
    );
    assert_eq!(
        unverified::frozen_reasons(params),
        [(Guarantee::BoundedSize, Some(WHY.to_string()))]
    );
    // There's no marker on the protocol, and nothing else is given up.
    assert_eq!(unverified::marker(params), None);
}

#[test]
fn the_argument_keeps_its_marker_frozen_beside_it() {
    let (units, _) = built(&format!(
        "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string, n: u32);\n}}\n"
    ));
    let arguments = units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Protocol { functions, .. } => match &functions[0] {
                FrozenUnit::Function { arguments, .. } => Some(arguments.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    assert_eq!(
        unverified::marker(&arguments[0].parameters),
        Some(&ConfigValue::Dict(vec![(
            "bounded_size".into(),
            ConfigValue::Str(WHY.into())
        )]))
    );
    assert!(arguments[1].parameters.is_empty());
}

// (b)
#[test]
fn a_field_marker_covers_a_struct_the_protocol_reaches() {
    let (units, _) = built(&format!(
        "struct Blob {{\n    @unverified(bounded_size = \"{WHY}\")\n    data: string\n}}\n\n\
         protocol P {{\n    function f(b: Blob);\n}}\n"
    ));
    assert_eq!(
        unverified::frozen_set(params(&units, "P")),
        [Guarantee::BoundedSize]
    );
    assert_eq!(
        at(&units, "P"),
        [(Scope::Field, "Blob.data".to_string(), WHY.to_string())]
    );
}

#[test]
fn a_field_marker_covers_only_the_field() {
    let err = error(&format!(
        "struct Blob {{\n    @unverified(bounded_size = \"{WHY}\")\n    data: string\n    name: string\n}}\n\n\
         protocol P {{\n    function f(b: Blob);\n}}\n"
    ));
    assert!(err.contains("bounds::unbounded"), "{err}");
    assert!(err.contains("field `name`"), "{err}");
    assert!(!err.contains("field `data`"), "{err}");
}

// (c)
#[test]
fn a_type_marker_covers_every_use_in_the_package() {
    let (units, _) = built(&format!(
        "@unverified(bounded_size = \"{WHY}\")\ntype Blob = string\n\n\
         struct Holder {{\n    blob: Blob\n}}\n\n\
         protocol P {{\n    function f(b: Blob);\n    function g(h: Holder);\n}}\n\n\
         protocol Q {{\n    function h(b: Blob);\n}}\n"
    ));
    let expected = [(Scope::Type, "Blob".to_string(), WHY.to_string())];
    assert_eq!(at(&units, "P"), expected);
    assert_eq!(at(&units, "Q"), expected);
    assert_eq!(
        unverified::frozen_set(params(&units, "P")),
        [Guarantee::BoundedSize]
    );
}

#[test]
fn a_struct_marker_is_a_type_marker_too() {
    let (units, _) = built(&format!(
        "@unverified(bounded_size = \"{WHY}\")\nstruct Blob {{\n    data: string\n    name: string\n}}\n\n\
         protocol P {{\n    function f(b: Blob);\n}}\n"
    ));
    assert_eq!(
        at(&units, "P"),
        [(Scope::Type, "Blob".to_string(), WHY.to_string())]
    );
}

#[test]
fn a_type_in_another_schema_of_the_package_is_covered_by_its_marker() {
    let models =
        format!("@unverified(bounded_size = \"{WHY}\")\nstruct Blob {{\n    data: string\n}}\n");
    let api = "use models::Blob\n\nprotocol Orders {\n    function send(b: Blob);\n}\n";
    let ctx = PackageSources::new()
        .config(&config(ALLOW))
        .schema(["api"], api)
        .schema(["models"], &models)
        .compile()
        .unwrap_or_else(|e| panic!("{e:?}"));
    let units = ctx.schema_contexts[0]
        .borrow()
        .frozen_schema
        .borrow()
        .clone()
        .unwrap();
    assert_eq!(
        unverified::frozen_set(params(&units, "Orders")),
        [Guarantee::BoundedSize]
    );
    assert_eq!(
        at(&units, "Orders"),
        [(Scope::Type, "models::Blob".to_string(), WHY.to_string())]
    );
}

// (d)
#[test]
fn a_marker_on_one_argument_leaves_the_other_an_error() {
    let err = error(&format!(
        "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") a: string, b: string);\n}}\n"
    ));
    assert!(err.contains("bounds::unbounded"), "{err}");
    assert!(err.contains("argument `b`"), "{err}");
    assert!(err.contains("`P.f(b)`"), "{err}");
    assert!(!err.contains("argument `a`"), "{err}");
}

#[test]
fn a_marker_never_subtracts_from_what_another_path_reaches() {
    // `Blob` is reached through a marked argument and through an unmarked one.
    let err = error(&format!(
        "struct Blob {{\n    data: string\n}}\n\n\
         protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") a: Blob);\n    function g(b: Blob);\n}}\n"
    ));
    assert!(err.contains("bounds::unbounded"), "{err}");
    assert!(err.contains("`P.g(b) -> Blob.data`"), "{err}");
    assert!(!err.contains("P.f(a)"), "{err}");
}

// (e)
#[test]
fn a_guarantee_at_a_scope_it_doesnt_allow_is_an_error() {
    let err =
        error("protocol P {\n    function f(@unverified(schema_agreement = \"x\") a: u32);\n}\n");
    assert!(err.contains("unverified::wrong-scope"), "{err}");
    assert!(
        err.contains("`schema_agreement` can't be acknowledged at argument scope"),
        "{err}"
    );
    assert!(err.contains("package, protocol"), "{err}");

    // On a field and a type as well, and the keyed form's own spelling.
    let err = error("@unverified = { encoding_agreement = \"x\" }\nstruct S {\n    a: u32\n}\n");
    assert!(err.contains("unverified::wrong-scope"), "{err}");
}

#[test]
fn an_unknown_key_at_an_inner_scope_suggests_the_spelling() {
    let err =
        error("protocol P {\n    function f(@unverified(bounded_sise = \"x\") a: string);\n}\n");
    assert!(err.contains("unverified::unknown-guarantee"), "{err}");
    assert!(err.contains("did you mean `bounded_size`?"), "{err}");
}

// (f)
#[test]
fn a_plain_marker_on_an_argument_covers_only_bounded_size() {
    let schema = "@handshake = \"none\"\nprotocol P {\n    function f(@unverified = \"legacy blob\" data: string);\n}\n";
    let err = error(schema);
    // Bounded size is covered; the handshake's guarantees are not.
    assert!(err.contains("unverified::missing-marker"), "{err}");
    assert!(
        err.contains("schema agreement and encoding agreement"),
        "{err}"
    );
    assert!(!err.contains("bounds::unbounded"), "{err}");
    assert!(!err.contains("bounded size"), "{err}");

    // The protocol acknowledges those; the argument still covers its own.
    let (units, _) = built(&format!(
        "@handshake = \"none\"\n@unverified = {{ schema_agreement = \"a\", encoding_agreement = \"b\" }}\n\
         protocol P {{\n    function f(@unverified = \"{WHY}\" data: string);\n}}\n"
    ));
    let params = params(&units, "P");
    assert_eq!(
        unverified::frozen_set(params),
        [
            Guarantee::SchemaAgreement,
            Guarantee::EncodingAgreement,
            Guarantee::BoundedSize
        ]
    );
    assert_eq!(
        unverified::reason_of(params, Guarantee::BoundedSize),
        Some(WHY.to_string())
    );
    assert_eq!(
        unverified::reason_of(params, Guarantee::SchemaAgreement),
        Some("a".to_string())
    );
}

// (g)
#[test]
fn a_marker_on_something_already_bounded_gives_nothing_up() {
    let (units, warnings) = built(&format!(
        "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: char[max 8]);\n}}\n"
    ));
    assert!(
        warnings.contains(&"unverified::nothing-given-up".to_string()),
        "{warnings:?}"
    );
    assert!(unverified::frozen_set(params(&units, "P")).is_empty());
    assert!(at(&units, "P").is_empty());
}

// (h)
#[test]
fn a_protocol_marker_still_covers_everything() {
    let (units, warnings) = built(
        "struct Blob {\n    data: string\n}\n\n\
         @unverified = \"all of it\"\nprotocol P {\n    function f(a: string, b: Blob);\n    function g(c: u8[]);\n}\n",
    );
    assert_eq!(
        unverified::frozen_set(params(&units, "P")),
        [Guarantee::BoundedSize]
    );
    assert_eq!(
        at(&units, "P"),
        [(Scope::Protocol, "P".to_string(), "all of it".to_string())]
    );
    assert!(
        !warnings.contains(&"unverified::nothing-given-up".to_string()),
        "{warnings:?}"
    );
}

#[test]
fn a_keyed_protocol_marker_not_listing_it_is_still_the_validators_to_say() {
    let err = error(
        "@unverified = { version_honesty = \"x\" }\nprotocol P {\n    function f(s: string);\n}\n",
    );
    assert!(err.contains("unverified::missing-guarantee"), "{err}");
    assert!(!err.contains("bounds::unbounded"), "{err}");
}

#[test]
fn an_inner_marker_makes_a_protocol_markers_omission_no_error() {
    // Bounded size is acknowledged at the argument; the protocol's keyed
    // marker needn't list it.
    let (units, _) = built(
        "@handshake = \"none\"\n@unverified = { schema_agreement = \"a\", encoding_agreement = \"b\" }\n\
         protocol P {\n    function f(@unverified = \"x\" data: string);\n}\n",
    );
    assert_eq!(at(&units, "P").len(), 1);
}

// (i)
#[test]
fn inspect_shows_bounded_size_given_up_with_the_inner_reason() {
    let ctx = PackageSources::new()
        .config(&config(ALLOW))
        .schema(
            ["app"],
            &format!("protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string);\n}}\n"),
        )
        .compile()
        .unwrap();
    let [p] = inspect::protocols(&ctx).try_into().unwrap();
    assert!(p.unverified);
    let size = p
        .guarantees
        .iter()
        .find(|r| r.guarantee == Guarantee::BoundedSize)
        .unwrap();
    assert!(!size.held);
    assert_eq!(size.reason.as_deref(), Some(WHY));
    assert_eq!(size.causes, [unverified::UNBOUNDED_CAUSE]);
    for row in p
        .guarantees
        .iter()
        .filter(|r| r.guarantee != Guarantee::BoundedSize)
    {
        assert!(row.held, "{row:?}");
    }
}

// (j)
#[test]
fn the_diagnostic_names_the_path_to_the_cause() {
    let err = error("protocol Billing {\n    function upload(data: string);\n}\n");
    assert!(err.contains("bounds::unbounded"), "{err}");
    assert!(err.contains("`string` has no maximum"), "{err}");
    assert!(err.contains("`Billing.upload(data)`"), "{err}");

    let err = error(
        "struct Blob {\n    data: string\n}\n\nstruct Upload {\n    blob: Blob\n}\n\n\
         protocol Billing {\n    function upload(u: Upload);\n}\n",
    );
    assert!(
        err.contains("`Billing.upload(u) -> Upload.blob -> Blob.data`"),
        "{err}"
    );
    // And says where a narrower marker would go.
    assert!(
        err.contains("`@unverified(bounded_size = \"…\")` on field `data` of `Blob`"),
        "{err}"
    );
}

#[test]
fn a_type_nothing_reaches_must_still_be_bounded_unless_marked() {
    let err = error("struct Blob {\n    data: string\n}\n");
    assert!(err.contains("bounds::unbounded"), "{err}");
    let (_, warnings) = built(&format!(
        "struct Blob {{\n    @unverified(bounded_size = \"{WHY}\")\n    data: string\n}}\n"
    ));
    assert!(
        !warnings.contains(&"unverified::nothing-given-up".to_string()),
        "{warnings:?}"
    );
}

#[test]
fn markers_below_the_protocol_are_off_by_default_too() {
    let schema = format!(
        "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string);\n}}\n"
    );
    let err = format!("{:?}", compile(&config(""), &schema).unwrap_err());
    assert!(err.contains("annotations.unverified.allowed"), "{err}");
    let schema = format!("@unverified(bounded_size = \"{WHY}\")\ntype Blob = string\n");
    let err = format!("{:?}", compile(&config(""), &schema).unwrap_err());
    assert!(err.contains("annotations.unverified.allowed"), "{err}");
}

#[test]
fn a_function_marker_covers_its_arguments_and_return() {
    let (units, _) = built(&format!(
        "protocol P {{\n    @unverified(bounded_size = \"{WHY}\")\n    function f(a: string) -> string;\n    function g(b: u32);\n}}\n"
    ));
    assert_eq!(
        at(&units, "P"),
        [(Scope::Function, "P.f".to_string(), WHY.to_string())]
    );
}

// The frozen shape: additive, so no IR format change.

fn cbor<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut out = vec![];
    ciborium::into_writer(value, &mut out).unwrap();
    out
}

#[test]
fn an_argument_without_markers_freezes_to_the_bytes_it_always_did() {
    let plain = FrozenArgument {
        name: "a".into(),
        ty: comline_core::schema::ir::frozen::types::TypeRef::Unit,
        span: (0, 1),
        parameters: vec![],
    };
    let bytes = cbor(&plain);
    assert!(!String::from_utf8_lossy(&bytes).contains("parameters"));
    let back: FrozenArgument = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(back, plain);

    let marked = FrozenArgument {
        parameters: vec![FrozenUnit::Property {
            name: "unverified".into(),
            value: ConfigValue::Str("x".into()),
        }],
        ..plain
    };
    let back: FrozenArgument = ciborium::from_reader(cbor(&marked).as_slice()).unwrap();
    assert_eq!(back, marked);
}

#[test]
fn the_scope_record_roundtrips_and_is_read_in_order() {
    let mut parameters = vec![];
    let ack = |scope, path: &str| Acknowledgement {
        guarantee: Guarantee::BoundedSize,
        scope,
        path: path.into(),
        reason: format!("why {path}"),
    };
    unverified::acknowledge(
        &mut parameters,
        [ack(Scope::Argument, "P.f(a)"), ack(Scope::Protocol, "P")],
    );
    // Acknowledging the same place twice records it once.
    unverified::acknowledge(&mut parameters, [ack(Scope::Protocol, "P")]);
    let back: Vec<FrozenUnit> = ciborium::from_reader(cbor(&parameters).as_slice()).unwrap();
    assert_eq!(
        unverified::acknowledgements(&back),
        [ack(Scope::Protocol, "P"), ack(Scope::Argument, "P.f(a)")]
    );
}

#[test]
fn a_scope_record_a_schema_writes_itself_is_dropped() {
    let (units, _) = built(&format!(
        "@unverified_scopes = {{ bounded_size = \"forged\" }}\n\
         protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") a: string);\n}}\n"
    ));
    assert_eq!(
        at(&units, "P"),
        [(Scope::Argument, "P.f(a)".to_string(), WHY.to_string())]
    );
}
