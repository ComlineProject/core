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

#[test]
fn a_truncated_config_idp_still_reports_a_line_even_with_no_named_token() {
    // A block value opened but never closed: the parse error tree bottoms
    // out in an empty `FailedNode` that names no token at all, so this used
    // to fall all the way back to a bare "unrecognized or incomplete
    // syntax" with no location whatsoever. `find_unclosed_bracket` now
    // catches this directly - a strictly more specific result than the
    // generic fallback.
    let source = "congregation test\ndeps = {\n";

    let err = ProjectInterpreter::from_config_source(source)
        .expect_err("an unclosed `{` must not parse");

    let message = format!("{err:#}");
    assert!(!message.contains("ParseError {"), "{message}");
    assert!(!message.contains("FailedNode("), "{message}");
    assert!(message.contains("config.idp"), "{message}");
    assert!(message.contains("unclosed `{`"), "{message}");
    // A line:column-shaped location, e.g. "config.idp:2:1".
    assert!(message.contains(":2:"), "{message}");
}

#[test]
fn a_dangling_open_brace_points_at_itself_not_the_rest_of_the_file() {
    // The exact shape of the reported regression: a `code_generation`
    // dictionary opened but never closed, followed by several more lines
    // (blank lines, commented-out blocks) before EOF. Before
    // `find_unclosed_bracket`, this rendered a span covering the entire
    // file (line 1 to the last line) instead of pointing at the actual
    // mistake on the `code_generation = {` line.
    let source = "congregation test\n\
                   specification_version = 1\n\
                   \n\
                   code_generation = {\n\
                   \x20   languages = {\n\
                   \x20       rust#1.70.0 = {}\n\
                   \x20   }\n\
                   \n\
                   /*\n\
                   dependencies = {\n\
                   \x20   std = {}\n\
                   }\n\
                   */\n";

    let err = ProjectInterpreter::from_config_source(source)
        .expect_err("a dangling `code_generation = {` must not parse");

    let message = format!("{err:#}");
    assert!(message.contains("unclosed `{`"), "{message}");
    // Points at line 4 (`code_generation = {`), not line 1 or the last line.
    assert!(message.contains(":4:"), "{message}");
    assert!(!message.contains(":1:1"), "{message}");
}
