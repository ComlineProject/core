// ariadne-based diagnostics rendering tests: assert on the span/byte-range
// data that's actually load-bearing, not on ariadne's colored terminal
// output (which isn't stable/meaningful to snapshot).

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::context::SchemaContext;
use comline_core::schema::ir::diagnostics::render_validation_error;
use comline_core::schema::ir::validation::validate;
use comline_core::utils::codemap::CodeMap;

fn schema_context_for(source: &str) -> SchemaContext {
    let document = grammar::parse(source).expect("source should parse");
    let mut codemap = CodeMap::new();
    codemap.insert_file("test.ids".to_string(), source.to_string());
    SchemaContext::with_declarations(document.0, vec!["test".to_string()], codemap)
}

#[test]
fn test_duplicate_name_diagnostic_renders_with_span() {
    let code = "struct User {\n    id: u64\n}\n\nstruct User {\n    name: str\n}\n";
    let schema_context = schema_context_for(code);
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    assert!(errors[0].span.is_some());

    let rendered = render_validation_error(&errors[0], &schema_context);
    assert!(!rendered.is_empty());
    // The rendered diagnostic should surface the message and quote the
    // offending source, not just silently fall back to a bare string.
    assert!(rendered.contains("Duplicate definition of 'User'"));
    assert!(rendered.contains("struct User"));
}

#[test]
fn test_unknown_type_diagnostic_renders_with_span() {
    let code = "struct Post {\n    author: Author\n}\n";
    let schema_context = schema_context_for(code);
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    assert!(errors[0].span.is_some());

    let rendered = render_validation_error(&errors[0], &schema_context);
    assert!(!rendered.is_empty());
    assert!(rendered.contains("Unknown type 'Author'"));
    assert!(rendered.contains("author: Author"));
}

#[test]
fn test_diagnostic_without_span_falls_back_gracefully() {
    use comline_core::schema::ir::validation::ValidationError;

    let schema_context = schema_context_for("struct Empty {\n}\n");
    let error = ValidationError {
        message: "Synthetic error with no span".to_string(),
        context: "nowhere in particular".to_string(),
        span: None,
    };

    // Should not panic even though there's no span to render a label for.
    let rendered = render_validation_error(&error, &schema_context);
    assert!(rendered.contains("Synthetic error with no span"));
}
