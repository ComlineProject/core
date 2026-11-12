//! Strict mode for `@unverified` (core#222, item 2): `annotations.unverified.strict
//! = true` requires the keyed form, and each acknowledgement at the innermost
//! scope its guarantee allows on the path to its cause. Off (the default), an
//! outer marker covers what is inside it, and the narrower placement is only
//! suggested, as a hint.

use comline_catalog::Scope;
use comline_core::diagnostics::Severity;
use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::{FrozenUnit, Guarantee};
use comline_core::schema::ir::unverified;

const ALLOW: &str = "settings = {\n    annotations = {\n        unverified = {\n            allowed = true\n        }\n    }\n}\n";

const STRICT: &str = "settings = {\n    annotations = {\n        unverified = {\n            allowed = true\n            strict = true\n        }\n    }\n}\n";

const WHY: &str = "legacy blob";

fn config(extra: &str) -> String {
    format!(
        "congregation app\nspecification_version = 1\n\
         code_generation = {{ languages = {{ rust#1.70.0 = {{}} }} }}\n{extra}"
    )
}

/// What a build gave: the frozen units, the warnings' codes, and the hints'
/// (code, severity).
struct Built {
    units: Vec<FrozenUnit>,
    warnings: Vec<String>,
    hints: Vec<(String, Severity)>,
}

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
    let hints = schema
        .hints
        .borrow()
        .iter()
        .map(|h| (h.code.id.to_string(), h.effective_severity()))
        .collect();
    Ok(Built {
        units,
        warnings,
        hints,
    })
}

/// A build under `settings`, which has to succeed.
fn built(settings: &str, schema: &str) -> Built {
    compile(&config(settings), schema).unwrap_or_else(|e| panic!("{e:?}"))
}

/// A build under `settings`, which has to fail: its error, whole.
fn error(settings: &str, schema: &str) -> String {
    match compile(&config(settings), schema) {
        Ok(_) => panic!("the build should fail:\n{schema}"),
        Err(e) => format!("{e:?}"),
    }
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

/// Where the protocol's bounded size was acknowledged: scope and path.
fn at(units: &[FrozenUnit], protocol: &str) -> Vec<(Scope, String)> {
    unverified::acknowledgements(params(units, protocol))
        .into_iter()
        .filter(|a| a.guarantee == Guarantee::BoundedSize)
        .map(|a| (a.scope, a.path))
        .collect()
}

fn has(err: &str, code: &str) -> bool {
    err.contains(&format!("[{code}]")) || err.contains(code)
}

// (a) Strict off: item 1, unchanged.

const OUTER: &str =
    "@unverified(bounded_size = \"legacy blob\")\nprotocol P {\n    function f(data: string, n: u32);\n}\n";

#[test]
fn off_by_default_an_outer_marker_still_covers_what_is_inside_it() {
    for settings in [
        ALLOW,
        // Written out, it's the same.
        "settings = {\n    annotations = {\n        unverified = {\n            allowed = true\n            strict = false\n        }\n    }\n}\n",
    ] {
        // The keyed form on the protocol, as before.
        let b = built(settings, OUTER);
        assert_eq!(at(&b.units, "P"), [(Scope::Protocol, "P".to_string())]);
        // And the plain form.
        let b = built(
            settings,
            "@unverified = \"legacy blob\"\nprotocol P {\n    function f(data: string);\n}\n",
        );
        assert_eq!(
            unverified::frozen_set(params(&b.units, "P")),
            [Guarantee::BoundedSize]
        );
        assert_eq!(at(&b.units, "P"), [(Scope::Protocol, "P".to_string())]);
        // A marker where the cause is, as before.
        let b = built(
            settings,
            &format!(
                "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string);\n}}\n"
            ),
        );
        assert_eq!(
            at(&b.units, "P"),
            [(Scope::Argument, "P.f(data)".to_string())]
        );
        // Nothing here is strict's.
        assert!(b.warnings.iter().all(|w| !w.contains("strict")));
    }
}

#[test]
fn off_by_default_a_cause_no_marker_covers_is_still_the_same_error() {
    let err = error(ALLOW, "protocol P {\n    function f(data: string);\n}\n");
    assert!(err.contains("bounds::unbounded"), "{err}");
    assert!(err.contains("`P.f(data)`"), "{err}");
    assert!(!has(&err, "unverified::strict-scope"), "{err}");
}

// (b) The keyed form is required.

#[test]
fn strict_a_plain_marker_on_a_protocol_is_an_error_with_the_keyed_rewrite() {
    let schema = "@handshake = \"none\"\n@unverified = \"the gateway predates Comline\"\nprotocol P {\n    function f();\n}\n";
    let err = error(STRICT, schema);
    assert!(has(&err, "unverified::strict-keyed"), "{err}");
    assert!(
        err.contains("`@unverified` on protocol `P` is a plain reason"),
        "{err}"
    );
    // The help names what the plain marker covers at that spot.
    assert!(
        err.contains(
            "@unverified(schema_agreement = \"the gateway predates Comline\", encoding_agreement = \"the gateway predates Comline\")"
        ),
        "{err}"
    );

    // Off, the same schema builds.
    built(ALLOW, schema);
}

#[test]
fn strict_a_plain_marker_covering_bounded_size_says_so_in_the_rewrite() {
    let err = error(
        STRICT,
        "@unverified = \"legacy blob\"\nprotocol P {\n    function f(data: string);\n}\n",
    );
    assert!(has(&err, "unverified::strict-keyed"), "{err}");
    assert!(
        err.contains("@unverified(bounded_size = \"legacy blob\")"),
        "{err}"
    );
}

#[test]
fn strict_a_plain_marker_is_an_error_at_every_scope() {
    for (schema, on) in [
        (
            "protocol P {\n    @unverified = \"legacy blob\"\n    function f(data: string);\n}\n",
            "function `P.f`",
        ),
        (
            "protocol P {\n    function f(@unverified = \"legacy blob\" data: string);\n}\n",
            "argument `data` of `P.f`",
        ),
        (
            "struct Blob {\n    @unverified = \"legacy blob\"\n    data: string\n}\n\nprotocol P {\n    function f(b: Blob);\n}\n",
            "field `data` of `Blob`",
        ),
        (
            "@unverified = \"legacy blob\"\ntype Blob = string\n\nprotocol P {\n    function f(b: Blob);\n}\n",
            "type `Blob`",
        ),
    ] {
        let err = error(STRICT, schema);
        assert!(has(&err, "unverified::strict-keyed"), "{err}");
        assert!(err.contains(&format!("`@unverified` on {on} is a plain reason")), "{err}");
        assert!(
            err.contains("@unverified(bounded_size = \"legacy blob\")"),
            "{err}"
        );
        // Off, each builds.
        built(ALLOW, schema);
    }
}

// (c) The innermost scope is required.

#[test]
fn strict_a_protocol_marker_over_one_argument_names_the_argument() {
    let err = error(STRICT, OUTER);
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(
        err.contains("`bounded_size` is acknowledged on protocol `P`"),
        "{err}"
    );
    assert!(err.contains("argument `data` of `P.f`"), "{err}");
    assert!(err.contains("`P.f(data)`"), "{err}");
    // `n` holds nothing without a maximum, so nothing is asked of it.
    assert!(!err.contains("argument `n`"), "{err}");
    assert!(!has(&err, "unverified::strict-keyed"), "{err}");

    // Moved to the argument, it passes.
    let b = built(
        STRICT,
        &format!(
            "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string, n: u32);\n}}\n"
        ),
    );
    assert_eq!(
        at(&b.units, "P"),
        [(Scope::Argument, "P.f(data)".to_string())]
    );
}

#[test]
fn strict_names_every_narrower_element_a_marker_could_move_to() {
    let err = error(
        STRICT,
        "@unverified(bounded_size = \"legacy blob\")\nprotocol P {\n    function f(a: string, b: string);\n}\n",
    );
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(err.contains("argument `a` of `P.f`"), "{err}");
    assert!(err.contains("argument `b` of `P.f`"), "{err}");
}

#[test]
fn strict_a_marker_on_a_function_is_narrower_than_the_protocol_but_not_than_the_argument() {
    let schema = "protocol P {\n    @unverified(bounded_size = \"legacy blob\")\n    function f(data: string);\n}\n";
    let err = error(STRICT, schema);
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(
        err.contains("`bounded_size` is acknowledged on function `P.f`"),
        "{err}"
    );
    assert!(err.contains("argument `data` of `P.f`"), "{err}");

    // A return has no scope of its own: the function is the innermost.
    built(
        STRICT,
        "protocol P {\n    @unverified(bounded_size = \"legacy blob\")\n    function f() -> string;\n}\n",
    );
    let err = error(
        STRICT,
        "@unverified(bounded_size = \"legacy blob\")\nprotocol P {\n    function f() -> string;\n}\n",
    );
    assert!(err.contains("function `P.f`"), "{err}");
}

#[test]
fn strict_the_marker_goes_to_the_field_that_holds_the_value() {
    let schema = |on_struct: &str, on_field: &str| {
        format!(
            "{on_struct}struct Blob {{\n    {on_field}data: string\n    n: u32\n}}\n\nprotocol P {{\n    function f(b: Blob);\n}}\n"
        )
    };
    let marker = "@unverified(bounded_size = \"legacy blob\")";
    // On the struct, the field is narrower.
    let err = error(STRICT, &schema(&format!("{marker}\n"), ""));
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(err.contains("field `data` of `Blob`"), "{err}");
    // And on the argument.
    let err = error(
        STRICT,
        &format!(
            "struct Blob {{\n    data: string\n}}\n\nprotocol P {{\n    function f({marker} b: Blob);\n}}\n"
        ),
    );
    assert!(err.contains("field `data` of `Blob`"), "{err}");
    // On the field it passes.
    let b = built(STRICT, &schema("", &format!("{marker}\n    ")));
    assert_eq!(at(&b.units, "P"), [(Scope::Field, "Blob.data".to_string())]);
}

// (d) A type is the narrowest scope that covers a cause it holds.

#[test]
fn strict_a_marker_on_a_type_used_twice_is_innermost() {
    let b = built(
        STRICT,
        &format!(
            "@unverified(bounded_size = \"{WHY}\")\ntype Blob = string\n\n\
             protocol P {{\n    function f(a: Blob, b: Blob);\n}}\n"
        ),
    );
    assert_eq!(at(&b.units, "P"), [(Scope::Type, "Blob".to_string())]);
    assert!(b.hints.is_empty(), "{:?}", b.hints);

    // Marking each use instead is wider than the type that holds the value.
    let err = error(
        STRICT,
        &format!(
            "type Blob = string\n\n\
             protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") a: Blob, \
             @unverified(bounded_size = \"{WHY}\") b: Blob);\n}}\n"
        ),
    );
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(err.contains("type `Blob`"), "{err}");

    // So is the protocol's.
    let err = error(
        STRICT,
        &format!(
            "type Blob = string\n\n@unverified(bounded_size = \"{WHY}\")\n\
             protocol P {{\n    function f(a: Blob, b: Blob);\n}}\n"
        ),
    );
    assert!(err.contains("type `Blob`"), "{err}");
}

#[test]
fn strict_a_type_in_another_schema_of_the_package_is_judged_where_it_is_declared() {
    let models = format!("@unverified(bounded_size = \"{WHY}\")\ntype Blob = string\n");
    let api = "use models::Blob\n\nprotocol Orders {\n    function send(b: Blob);\n}\n";
    PackageSources::new()
        .config(config(STRICT))
        .schema(["api"], api)
        .schema(["models"], &models)
        .compile()
        .unwrap_or_else(|e| panic!("{e:?}"));
}

// (e) The handshake and conformance: the protocol is the innermost scope.

#[test]
fn strict_keyed_handshake_guarantees_on_the_protocol_pass() {
    let b = built(
        STRICT,
        "@handshake = \"none\"\n\
         @unverified = {\n    schema_agreement = \"the gateway pins schemas\",\n    \
         encoding_agreement = \"the gateway speaks one encoding\"\n}\n\
         protocol P {\n    function f();\n}\n",
    );
    assert_eq!(
        unverified::frozen_set(params(&b.units, "P")),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert!(b.hints.is_empty(), "{:?}", b.hints);
}

#[test]
fn strict_keeps_the_guarantees_each_marker_names_exact() {
    // Strict adds to what the keyed form already holds to: a computed
    // guarantee left out is still an error.
    let err = error(
        STRICT,
        "@handshake = \"none\"\n@unverified = { schema_agreement = \"x\" }\nprotocol P {\n    function f();\n}\n",
    );
    assert!(err.contains("unverified::missing-guarantee"), "{err}");
}

// (f) Package-default acknowledgements.

const PACKAGE_KEYED: &str =
    "handshake = \"none\"\nunverified = {\n    schema_agreement = \"the gateway pins schemas by contract\"\n    \
     encoding_agreement = \"the gateway pins schemas by contract\"\n}\n";

const BARE: &str = "protocol P {\n    function f();\n}\n";

#[test]
fn strict_package_default_keyed_acknowledgements_pass() {
    let b = built(&format!("{PACKAGE_KEYED}{STRICT}"), BARE);
    assert_eq!(
        unverified::frozen_set(params(&b.units, "P")),
        [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement]
    );
    assert!(unverified::package_scope(params(&b.units, "P")).is_some());
}

#[test]
fn strict_a_plain_package_reason_is_an_error_with_the_keyed_rewrite() {
    let plain = "handshake = \"none\"\nunverified = \"the gateway pins schemas by contract\"\n";
    let err = error(&format!("{plain}{STRICT}"), BARE);
    assert!(has(&err, "unverified::strict-keyed"), "{err}");
    assert!(
        err.contains("config.idp's `unverified` is a plain reason"),
        "{err}"
    );
    assert!(
        err.contains("unverified = { schema_agreement = \"the gateway pins schemas by contract\", encoding_agreement = \"the gateway pins schemas by contract\" }"),
        "{err}"
    );
    // Off, it builds.
    built(&format!("{plain}{ALLOW}"), BARE);
}

#[test]
fn strict_what_a_package_setting_gives_up_is_acknowledged_beside_the_setting() {
    // The protocol takes the package's `none` handshake, and says why itself:
    // the innermost scope of that cause is the package.
    let schema =
        "@unverified = {\n    schema_agreement = \"x\"\n    encoding_agreement = \"y\"\n}\n\
                  protocol P {\n    function f();\n}\n";
    let err = error(&format!("handshake = \"none\"\n{STRICT}"), schema);
    assert!(has(&err, "unverified::strict-scope"), "{err}");
    assert!(
        err.contains("`schema_agreement`, `encoding_agreement`"),
        "{err}"
    );
    assert!(err.contains("config.idp"), "{err}");
    // Off, it builds, as it always did.
    built(&format!("handshake = \"none\"\n{ALLOW}"), schema);

    // A protocol that writes its own handshake is the cause itself: the
    // protocol is where it sits, so the protocol's is innermost.
    built(
        &format!("handshake = \"none\"\n{STRICT}"),
        "@handshake = \"none\"\n@unverified = {\n    schema_agreement = \"x\"\n    encoding_agreement = \"y\"\n}\n\
         protocol P {\n    function f();\n}\n",
    );
}

// (g) Off: the narrower placement is a hint.

#[test]
fn off_an_outer_marker_that_could_be_narrower_draws_a_hint_and_nothing_else() {
    let b = built(ALLOW, OUTER);
    assert_eq!(
        b.hints,
        [("unverified::narrower-placement".to_string(), Severity::Hint)]
    );
    // Not a warning: what a build warns of is unchanged.
    assert!(
        !b.warnings.iter().any(|w| w.contains("narrower")),
        "{:?}",
        b.warnings
    );
    // And what is frozen is unchanged.
    assert_eq!(at(&b.units, "P"), [(Scope::Protocol, "P".to_string())]);
}

#[test]
fn off_the_hint_names_where_the_marker_could_go() {
    let ctx = PackageSources::new()
        .config(config(ALLOW))
        .schema(["app"], OUTER)
        .compile()
        .unwrap();
    let schema = ctx.schema_contexts[0].borrow();
    let hints = schema.hints.borrow();
    assert_eq!(hints.len(), 1);
    let hint = &hints[0];
    assert!(hint.message.contains("protocol `P`"), "{}", hint.message);
    assert!(
        hint.message.contains("argument `data` of `P.f`"),
        "{}",
        hint.message
    );
    assert!(
        hint.help
            .as_deref()
            .unwrap_or("")
            .contains("annotations.unverified.strict"),
        "{:?}",
        hint.help
    );
}

#[test]
fn off_a_marker_already_at_the_innermost_scope_draws_no_hint() {
    let b = built(
        ALLOW,
        &format!(
            "protocol P {{\n    function f(@unverified(bounded_size = \"{WHY}\") data: string);\n}}\n"
        ),
    );
    assert!(b.hints.is_empty(), "{:?}", b.hints);
    let b = built(
        ALLOW,
        &format!("@unverified(bounded_size = \"{WHY}\")\ntype Blob = string\n\nprotocol P {{\n    function f(a: Blob);\n}}\n"),
    );
    assert!(b.hints.is_empty(), "{:?}", b.hints);
}

// Where it is set.

#[test]
fn strict_can_be_set_in_a_schemas_own_settings() {
    let schema = format!("{OUTER}\nsettings {{\n    annotations.unverified.strict = true\n}}\n");
    let err = error(ALLOW, &schema);
    assert!(has(&err, "unverified::strict-scope"), "{err}");
}

#[test]
fn strict_can_be_scoped_to_a_declaration_site() {
    // On protocols only: a plain marker on a type is still fine.
    let settings = "settings = {\n    annotations = {\n        unverified = {\n            allowed = true\n        }\n    }\n    \
                    protocol = {\n        annotations = {\n            unverified = {\n                strict = true\n            }\n        }\n    }\n}\n";
    let err = error(
        settings,
        "@unverified = \"legacy blob\"\nprotocol P {\n    function f(data: string);\n}\n",
    );
    assert!(has(&err, "unverified::strict-keyed"), "{err}");
    built(
        settings,
        "@unverified = \"legacy blob\"\ntype Blob = string\n\nprotocol P {\n    function f(a: Blob);\n}\n",
    );
}

// (h) The setting is validated.

#[test]
fn the_setting_takes_a_boolean() {
    for value in ["\"yes\"", "1"] {
        let settings = format!(
            "settings = {{\n    annotations = {{\n        unverified = {{\n            allowed = true\n            strict = {value}\n        }}\n    }}\n}}\n"
        );
        let err = error(&settings, BARE);
        assert!(has(&err, "settings::malformed-value"), "{err}");
        assert!(err.contains("annotations.unverified.strict"), "{err}");
    }
}

#[test]
fn an_unknown_sibling_key_is_still_an_unknown_key() {
    let settings = "settings = {\n    annotations = {\n        unverified = {\n            allowed = true\n            stricct = true\n        }\n    }\n}\n";
    let err = error(settings, BARE);
    assert!(has(&err, "settings::unknown-key"), "{err}");
    assert!(
        err.contains("annotations.unverified.strict"),
        "should suggest the key: {err}"
    );
}

#[test]
fn only_unverified_has_a_strict_key() {
    let settings = "settings = {\n    annotations = {\n        framing = {\n            strict = true\n        }\n    }\n}\n";
    let err = error(settings, BARE);
    assert!(has(&err, "settings::unknown-key"), "{err}");
}
