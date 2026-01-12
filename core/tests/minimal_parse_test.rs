// Minimal stdlib test - fixed compilation errors
use comline_core::schema::idl::grammar;

#[test]
fn test_minimal_working() {
    let code = "struct User { name: str }";
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Basic struct should parse");
}

#[test]
fn test_protocol_only() {
    let code = r#"
protocol TestOps {
    function new() -> u64;
}
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Protocol should parse");
}

#[test]
fn test_struct_and_protocol() {
    let code = r#"
struct HashMap {
    capacity: u64;
}

protocol HashMapOps {
    function new() -> HashMap;
}
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Struct + Protocol should parse");
}
