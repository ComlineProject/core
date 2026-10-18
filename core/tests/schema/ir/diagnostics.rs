// ariadne-based diagnostics rendering tests: assert on the span/byte-range
// data that's actually load-bearing, not on ariadne's colored terminal
// output (which isn't stable/meaningful to snapshot).

use comline_core::diagnostics::{render, Diagnostic, DiagnosticCode};
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::validation::validate;

#[test]
fn test_duplicate_name_diagnostic_renders_with_span() {
    let code = "struct User {\n    id: u64\n}\n\nstruct User {\n    name: string\n}\n";
    let _ = grammar::parse(code).expect("source should parse");
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    assert!(errors[0].span.is_some());

    let rendered = render(&errors[0], "test.ids", code);
    assert!(!rendered.is_empty());
    // The rendered diagnostic should surface the message and quote the
    // offending source, not just silently fall back to a bare string.
    assert!(rendered.contains("Duplicate definition of 'User'"));
    assert!(rendered.contains("struct User"));
    assert_eq!(errors[0].code.id, "validation::duplicate-declaration");
}

#[test]
fn test_unknown_type_diagnostic_renders_with_span() {
    let code = "struct Post {\n    author: Author\n}\n";
    let _ = grammar::parse(code).expect("source should parse");
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    assert!(errors[0].span.is_some());

    let rendered = render(&errors[0], "test.ids", code);
    assert!(!rendered.is_empty());
    assert!(rendered.contains("Unknown type 'Author'"));
    assert!(rendered.contains("author: Author"));
    assert_eq!(errors[0].code.id, "validation::unknown-type");
}

#[test]
fn test_diagnostic_without_span_falls_back_gracefully() {
    let code = DiagnosticCode::error("test::synthetic");
    let error = Diagnostic::new(code, "Synthetic error with no span").with_context("nowhere in particular");

    // Should not panic even though there's no span to render a label for.
    let rendered = render(&error, "test.ids", "struct Empty {\n}\n");
    assert!(rendered.contains("Synthetic error with no span"));
}
