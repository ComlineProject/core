//! `PackageSources` — compiling a package from in-memory strings, no filesystem.

use comline_core::package::build::PackageSources;

#[test]
fn compiles_a_single_schema_with_a_synthesised_congregation() {
    let ctx = PackageSources::new()
        .schema(
            ["chat"],
            "struct Message {\n    body: str\n}\n",
        )
        .compile()
        .expect("should compile");

    assert!(ctx.config_frozen.is_some(), "config_frozen must be set");
    assert_eq!(ctx.schema_contexts.len(), 1);

    let schema = ctx.schema_contexts[0].borrow();
    assert_eq!(schema.namespace, vec!["chat".to_string()]);
    let frozen = schema.frozen_schema.borrow();
    assert!(frozen.is_some(), "schema should have been interpreted");
}

#[test]
fn honours_an_explicit_congregation() {
    let config = "congregation my_app\nspecification_version = 1\n\n\
                  code_generation = {\n    languages = {\n        rust#1.70.0 = {}\n    }\n}\n";

    let ctx = PackageSources::new()
        .config(config)
        .schema(["ping"], "struct Ping {\n    seq: u32\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(ctx.config.name.value, "my_app");
}

#[test]
fn multiple_schemas_are_all_interpreted() {
    let ctx = PackageSources::new()
        .schema(["types"], "struct User {\n    id: u64\n}\n")
        .schema(["ping"], "struct Ping {\n    seq: u32\n}\n")
        .schema(["chat"], "struct Message {\n    body: str\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(ctx.schema_contexts.len(), 3);
    for sc in &ctx.schema_contexts {
        assert!(sc.borrow().frozen_schema.borrow().is_some());
    }
}

#[test]
fn cross_schema_use_resolves_across_the_added_schemas() {
    // The bare name (`use ns::Name` -> `Name`), the qualified form (`ns::Name`),
    // and an alias (`use ns::Name as X` -> `X`) all resolve.
    let cases = [
        ("use types::User", "User"),
        ("use types::User", "types::User"),
        ("use types::User as Account", "Account"),
    ];
    for (import, reference) in cases {
        let ctx = PackageSources::new()
            .schema(["types"], "struct User {\n    id: u64\n}\n")
            .schema(
                ["api"],
                &format!("{import}\n\nstruct Session {{\n    user: {reference}\n}}\n"),
            )
            .compile()
            .unwrap_or_else(|e| panic!("`{import}` / `{reference}` should resolve: {e}"));

        assert_eq!(ctx.schema_contexts.len(), 2);
        for sc in &ctx.schema_contexts {
            assert!(sc.borrow().frozen_schema.borrow().is_some());
        }
    }
}

#[test]
fn nested_namespace_segments_are_kept() {
    let ctx = PackageSources::new()
        .schema(["chat", "admin"], "struct Ban {\n    who: str\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(
        ctx.schema_contexts[0].borrow().namespace,
        vec!["chat".to_string(), "admin".to_string()]
    );
}

#[test]
fn a_parse_error_is_returned_not_panicked() {
    let err = PackageSources::new()
        .schema(["bad"], "struct {{{ not valid")
        .compile()
        .expect_err("should be an error");

    assert!(err.to_string().contains("bad"), "error names the schema: {err}");
}

#[test]
fn a_deprecated_struct_usage_compiles_successfully_and_is_a_warning() {
    let ctx = PackageSources::new()
        .schema(
            ["chat"],
            "@deprecated = \"use Message2 instead\"\n\
             struct Message {\n    body: str\n}\n\n\
             struct Envelope {\n    inner: Message\n}\n",
        )
        .compile()
        .expect("deprecation is a warning, not a build failure");

    let rendered = ctx.rendered_warnings();
    assert_eq!(rendered.len(), 1);
    assert!(rendered[0].contains("'Message' is deprecated"), "got: {}", rendered[0]);
    assert!(rendered[0].contains("use Message2 instead"), "got: {}", rendered[0]);

    let raw = ctx.warnings();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].code.id, "validation::deprecated-type-usage");
}

#[test]
fn no_deprecation_means_no_warnings() {
    let ctx = PackageSources::new()
        .schema(["chat"], "struct Message {\n    body: str\n}\n")
        .compile()
        .expect("should compile");

    assert!(ctx.warnings().is_empty());
    assert!(ctx.rendered_warnings().is_empty());
}

#[test]
fn settings_can_allow_away_a_deprecation_warning() {
    let ctx = PackageSources::new()
        .schema(
            ["chat"],
            "@deprecated\n\
             struct Message {\n    body: str\n}\n\n\
             struct Envelope {\n    inner: Message\n}\n\n\
             settings {\n    diagnostics.\"validation::deprecated-type-usage\".level = \"allow\"\n}\n",
        )
        .compile()
        .expect("should compile - allow just silences the warning, it isn't a build failure");

    assert!(ctx.warnings().is_empty(), "the allow-overridden warning should vanish entirely");
}

#[test]
fn settings_can_escalate_a_deprecation_warning_to_a_build_failure() {
    let err = PackageSources::new()
        .schema(
            ["chat"],
            "@deprecated\n\
             struct Message {\n    body: str\n}\n\n\
             struct Envelope {\n    inner: Message\n}\n\n\
             settings {\n    diagnostics.\"validation::deprecated-type-usage\".level = \"error\"\n}\n",
        )
        .compile()
        .expect_err("escalating to 'error' must fail the build - that's the whole point");

    assert!(err.to_string().contains("Message"), "error names the escalated diagnostic: {err}");
}

#[test]
fn a_wrongly_typed_annotation_value_fails_the_build() {
    // Core had no opinion on these at all before: both of these compiled
    // clean and were then silently dropped by whichever generator read
    // them.
    for (source, expected) in [
        ("@framing = \"nonsense\"\nprotocol P {\n    function f();\n}\n", "jsonrpc"),
        (
            "protocol P {\n    @timeout_ms = \"abc\"\n    function f();\n}\n",
            "an integer",
        ),
    ] {
        let err = PackageSources::new()
            .schema(["chat"], source)
            .compile()
            .expect_err("a wrongly-typed annotation value is a hard error");
        let text = err.to_string();
        assert!(text.contains("annotation-value-mismatch"), "got: {text}");
        assert!(text.contains(expected), "expected {expected:?} in: {text}");
    }
}

#[test]
fn a_framing_alias_is_accepted_everywhere_the_generators_accept_one() {
    // The alias set lives in core now, so every consumer agrees. The
    // playground's own copy was missing `json-rpc`.
    for spelling in ["jsonrpc", "json-rpc", "jsonrpc-2.0", "datagram"] {
        PackageSources::new()
            .schema(
                ["chat"],
                &format!(
                    "@framing = \"{spelling}\"\nprotocol P {{\n    function f();\n}}\n\
                     settings {{\n    annotations.framing.allowed = true\n}}\n"
                ),
            )
            .compile()
            .unwrap_or_else(|e| panic!("{spelling} should be accepted: {e}"));
    }
}

#[test]
fn an_unknown_annotation_is_still_not_an_error() {
    // `@key` is an open namespace by design - an unrecognised one has no
    // declared shape to check against.
    PackageSources::new()
        .schema(
            ["chat"],
            "@whatever = 42\nprotocol P {\n    function f();\n}\n",
        )
        .compile()
        .expect("an unrecognised annotation must not fail the build");
}

#[test]
fn a_misspelled_settings_key_fails_the_build_with_a_suggestion() {
    // Used to compile clean and do nothing at all.
    let err = PackageSources::new()
        .schema(
            ["chat"],
            "struct Message {\n    body: str\n}\n\n\
             settings {\n    annotations.framing.alowed = true\n}\n",
        )
        .compile()
        .expect_err("an unrecognised settings key is a hard error");

    let text = err.to_string();
    assert!(text.contains("settings::unknown-key"), "got: {text}");
    assert!(text.contains("annotations.framing.allowed"), "should suggest the fix: {text}");
}

#[test]
fn an_unknown_top_level_settings_key_still_builds() {
    // `settings` stays an open namespace outside the groups core owns.
    PackageSources::new()
        .schema(
            ["chat"],
            "struct Message {\n    body: str\n}\n\n\
             settings {\n    forbid_indexing = true\n    max_depth = 8\n}\n",
        )
        .compile()
        .expect("an open-namespace key must not fail the build");
}

#[test]
fn a_wrongly_typed_settings_value_fails_even_when_nothing_uses_it() {
    // No validator is referenced anywhere, so `enforcement` never looks at
    // this key - it used to build clean.
    let err = PackageSources::new()
        .schema(
            ["chat"],
            "struct Message {\n    body: str\n}\n\n\
             settings {\n    validators.allowed = Tru\n}\n",
        )
        .compile()
        .expect_err("a wrongly-typed settings value is a hard error");

    let text = err.to_string();
    assert!(text.contains("must be a boolean"), "got: {text}");
    assert!(!text.contains("Identifier("), "no Debug dump in user text: {text}");
}

#[test]
fn a_malformed_diagnostic_level_is_its_own_build_failure() {
    let err = PackageSources::new()
        .schema(
            ["chat"],
            "@deprecated\n\
             struct Message {\n    body: str\n}\n\n\
             struct Envelope {\n    inner: Message\n}\n\n\
             settings {\n    diagnostics.\"validation::deprecated-type-usage\".level = \"nope\"\n}\n",
        )
        .compile()
        .expect_err("a malformed level value is a hard error, not silently ignored");

    assert!(err.to_string().contains("invalid-diagnostic-level"), "got: {err}");
}
