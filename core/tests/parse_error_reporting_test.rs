//! Regression tests for the reported bug: a malformed schema or
//! `config.idp` used to surface as a raw `Debug`-dumped `Vec<ParseError>`
//! (`[ParseError { reason: FailedNode(...), start: ..., end: ... }, ...]`)
//! instead of a human-readable, line/column-located message.

use comline_core::package::build::PackageSources;
use comline_core::package::config::ir::interpreter::ProjectInterpreter;

#[test]
fn a_malformed_schema_reports_a_human_readable_error_not_a_debug_dump() {
    // A stray `/` where a field's type should be - the same shape of
    // break as the originally reported bug.
    let source = "struct Greeting {\n    / name: string\n}\n";

    let err = PackageSources::new()
        .schema(["main"], source)
        .compile()
        .expect_err("a stray `/` must not parse");

    let message = format!("{err:#}");
    assert!(!message.contains("ParseError {"), "{message}");
    assert!(!message.contains("FailedNode("), "{message}");
    assert!(message.contains("main.ids"), "{message}");
    assert!(message.contains("unexpected token"), "{message}");
    // A line:column-shaped location, e.g. "main.ids:2:5".
    assert!(message.contains(":2:"), "{message}");
}

#[test]
fn a_malformed_config_idp_reports_a_human_readable_error_not_a_debug_dump() {
    let source = "congregation test\nspecification_version =\n";

    let err = ProjectInterpreter::from_config_source(source)
        .expect_err("a dangling `=` with no value must not parse");

    let message = format!("{err:#}");
    assert!(!message.contains("ParseError {"), "{message}");
    assert!(!message.contains("FailedNode("), "{message}");
    assert!(message.contains("config.idp"), "{message}");
}
