//! End-to-end: a declaration using a forbidden annotation/validator, or a
//! malformed settings block, actually fails a real `PackageSources::compile()`
//! - the thing `core::settings::effective` always computed correctly but
//! nothing ever consumed until now.

use comline_core::package::build::PackageSources;

#[test]
fn a_forbidden_annotation_fails_compilation() {
    // `.idp` has no dotted-key sugar (that's `.ids`-only) - a nested key
    // here needs real nested braces.
    let result = PackageSources::new()
        .config(
            r#"
congregation acme
specification_version = 1
code_generation = { languages = { rust#1.70.0 = {} } }
settings = {
    protocol = {
        annotations = {
            framing = {
                allowed = false
            }
        }
    }
}
"#,
        )
        .schema(
            ["chat"],
            r#"
@framing = "jsonrpc"
protocol Chat {
    function ping() -> str;
}
"#,
        )
        .compile();

    let err = result.expect_err("a forbidden @framing should fail compilation");
    let message = format!("{err}");
    assert!(message.contains("framing"), "error should name the annotation: {message}");
    assert!(
        message.contains("forbidden") && message.contains("settings enforcement"),
        "error should come from settings enforcement, not some other cause: {message}"
    );
}

#[test]
fn an_allowed_annotation_compiles_fine() {
    let result = PackageSources::new()
        .config(
            r#"
congregation acme
specification_version = 1
code_generation = { languages = { rust#1.70.0 = {} } }
"#,
        )
        .schema(
            ["chat"],
            r#"
@framing = "jsonrpc"
protocol Chat {
    function ping() -> str;
}
"#,
        )
        .compile();

    assert!(result.is_ok(), "no package settings forbid anything here: {:?}", result.err());
}

#[test]
fn a_leaf_group_conflict_now_fails_compilation() {
    // Regression test for a real bug: `check_settings_conflicts` existed
    // but was never wired into the real build path - this schema used to
    // compile silently (first value wins, no error).
    let result = PackageSources::new()
        .config(
            r#"
congregation acme
specification_version = 1
code_generation = { languages = { rust#1.70.0 = {} } }
"#,
        )
        .schema(
            ["chat"],
            r#"
settings {
    a.b = True
    a.b.c = 1
}
"#,
        )
        .compile();

    let err = result.expect_err("a leaf-vs-group conflict should fail compilation");
    assert!(format!("{err}").contains("a.b"));
}

#[test]
fn an_invalid_mode_value_now_fails_compilation() {
    let result = PackageSources::new()
        .config(
            r#"
congregation acme
specification_version = 1
code_generation = { languages = { rust#1.70.0 = {} } }
"#,
        )
        .schema(["chat"], "settings {\n    mode = merge\n}")
        .compile();

    let err = result.expect_err("an invalid mode value should fail compilation");
    assert!(format!("{err}").to_lowercase().contains("mode"));
}
