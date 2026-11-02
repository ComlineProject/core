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

/// The errors `validate` reports for `code`, as `(code, offending text)`.
fn duplicates(code: &str) -> Vec<(String, String)> {
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    errors
        .iter()
        .filter(|e| e.code.id.starts_with("validation::duplicate-"))
        .map(|e| {
            let (start, end) = e.span.expect("a duplicate points at its repeat");
            assert!(e.secondary.is_some(), "and back at the first: {e:?}");
            (e.code.id.to_string(), code[start..end].lines().next().unwrap().trim().to_string())
        })
        .collect()
}

#[test]
fn duplicate_names_inside_a_declaration_are_errors() {
    assert_eq!(
        duplicates("struct S {\n    a: u8\n    a: u16\n}\n"),
        [("validation::duplicate-field".into(), "a: u16".into())]
    );
    assert_eq!(
        duplicates("error F {\n    message = \"x\"\n    a: u8\n    a: u8\n}\n")[0].0,
        "validation::duplicate-field"
    );
    assert_eq!(
        duplicates("enum E {\n    A\n    B\n    A\n}\n"),
        [("validation::duplicate-variant".into(), "A".into())]
    );
    assert_eq!(
        duplicates("struct S {\n    a: u8\n}\nprotocol P {\n    function f(s: S) -> u32;\n    function f() -> u32;\n}\n"),
        [("validation::duplicate-function".into(), "function f() -> u32;".into())]
    );
    assert_eq!(
        duplicates("struct S {\n    a: u8\n}\nprotocol P {\n    function f(a: S, a: S) -> u32;\n}\n")[0].0,
        "validation::duplicate-argument"
    );
    assert_eq!(
        duplicates(
            "validator V for string {\n    max: u32 = 1\n    max: u32 = 2\n    validate {\n        \
             assert(value.length <= params.max, f\"{value.name} too long\")\n    }\n}\n"
        )[0]
        .0,
        "validation::duplicate-parameter"
    );
}

#[test]
fn distinct_names_and_unnamed_arguments_are_not_duplicates() {
    let code = "struct S {\n    a: u8\n    b: u8\n}\nenum E {\n    A\n    B\n}\n\
                protocol P {\n    function f(S, S) -> u32;\n    function g(a: S, b: S) -> u32;\n}\n";
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir));
}
