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

#[test]
fn an_error_colliding_with_a_type_is_a_duplicate_declaration() {
    // Both generate a type named after themselves - `export interface Test`
    // in TypeScript, `pub struct Test` in Rust - so this emitted two
    // definitions of `Test` and the generated code did not compile. Core
    // excluded errors from the duplicate check outright, so nothing said so.
    for (label, source) in [
        ("struct", "struct Test {\n    a: str\n}\n\nerror Test {\n    message = \"\"\n}\n"),
        ("enum", "enum Test {\n    A\n}\n\nerror Test {\n    message = \"\"\n}\n"),
        (
            "protocol",
            "protocol Test {\n    function f();\n}\n\nerror Test {\n    message = \"\"\n}\n",
        ),
        // Two errors were not caught either.
        (
            "another error",
            "error Test {\n    message = \"\"\n}\n\nerror Test {\n    message = \"\"\n}\n",
        ),
    ] {
        let err = PackageSources::new()
            .schema(["chat"], source)
            .compile()
            .err()
            .unwrap_or_else(|| panic!("{label} + error Test should be rejected"));
        let text = err.to_string();
        assert!(text.contains("Duplicate definition"), "{label}: {text}");
        assert!(text.contains("Test"), "{label}: {text}");
    }
}

#[test]
fn a_settings_preset_may_share_a_name_with_a_declaration() {
    // A `settings` block generates no type, so it cannot collide in codegen
    // and stays in its own namespace. This is the case the editor has to
    // disambiguate rather than the one core should reject.
    PackageSources::new()
        .schema(
            ["chat"],
            "struct Test {\n    a: str\n}\n\n\
             settings Test {\n    validators.allowed = true\n}\n",
        )
        .compile()
        .expect("a settings preset is a separate namespace");
}

#[test]
fn a_name_that_is_not_a_type_is_rejected_in_a_type_position() {
    // Resolution was presence-only, so anything in the symbol table passed
    // as a field's type. Each of these compiled clean.
    for (source, expected) in [
        (
            "error Boom {\n    message = \"\"\n}\n\nstruct S {\n    f: Boom\n}\n",
            "an error",
        ),
        (
            "protocol P {\n    function f();\n}\n\nstruct S {\n    f: P\n}\n",
            "a protocol",
        ),
        ("const LIMIT: u16 = 20\n\nstruct S {\n    f: LIMIT\n}\n", "a constant"),
    ] {
        let err = PackageSources::new()
            .schema(["chat"], source)
            .compile()
            .expect_err("a non-type in a type position is an error");
        let text = err.to_string();
        assert!(text.contains("not a type"), "got: {text}");
        assert!(text.contains(expected), "expected {expected:?} in: {text}");
    }
}

#[test]
fn an_error_is_still_usable_where_an_error_belongs() {
    // The flip side of the two tests above: folding errors into the
    // declaration namespace must not disturb `! Name` throws, which is the
    // one place an error is meant to be named.
    PackageSources::new()
        .schema(
            ["chat"],
            "error Rejected {\n    message = \"\"\n}\n\n\
             protocol Chat {\n    function send(text: str) ! Rejected;\n}\n",
        )
        .compile()
        .expect("a throws clause must still resolve its error");
}

#[test]
fn a_duplicate_settings_block_is_reported_rather_than_silently_dropped() {
    // `effective::schema_settings` keys named blocks by name and keeps a
    // single unnamed slot, so a second block with the same name replaced
    // the first outright - everything written in the first was discarded
    // with no diagnostic anywhere.
    for (label, source) in [
        (
            "named",
            "struct S {\n    a: str\n}\n\n\
             settings Strict {\n    validators.allowed = true\n}\n\n\
             settings Strict {\n    annotations.framing.allowed = true\n}\n",
        ),
        (
            "unnamed",
            "struct S {\n    a: str\n}\n\n\
             settings {\n    validators.allowed = true\n}\n\n\
             settings {\n    annotations.framing.allowed = true\n}\n",
        ),
    ] {
        let err = PackageSources::new()
            .schema(["chat"], source)
            .compile()
            .err()
            .unwrap_or_else(|| panic!("a duplicate {label} block should be reported"));
        let text = err.to_string();
        assert!(text.contains("settings::duplicate-block"), "{label}: {text}");
        assert!(text.contains("only the last one"), "{label}: {text}");
    }
}

#[test]
fn distinct_settings_blocks_are_still_fine() {
    PackageSources::new()
        .schema(
            ["chat"],
            "struct S {\n    a: str\n}\n\n\
             settings {\n    validators.allowed = true\n}\n\n\
             settings Strict {\n    annotations.framing.allowed = true\n}\n\n\
             settings Lax {\n    validators.allowed = false\n}\n",
        )
        .compile()
        .expect("one unnamed block plus distinct named blocks is the normal case");
}
