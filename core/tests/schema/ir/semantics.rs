use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::validation::validate;

#[test]
fn test_duplicate_definition_error() {
    let code = r#"
struct User {
    id: u64
}

struct User {
    name: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Duplicate definition of 'User'"));
}

#[test]
fn test_unknown_type_error() {
    let code = r#"
struct Post {
    author: Author  // Author is not defined
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Unknown type 'Author'"));
}

#[test]
fn test_valid_schema_passes() {
    let code = r#"
struct Author {
    id: u64
    name: str
}

struct Post {
    id: u64
    author: Author
    comments: str[]
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_ok());
}

#[test]
fn test_protocol_unknown_arg_type() {
    let code = r#"
protocol Service {
    function get(UnknownType) returns bool
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    assert!(result.unwrap_err()[0].message.contains("Unknown type 'UnknownType'"));
}

#[test]
fn test_protocol_unknown_return_type() {
    let code = r#"
protocol Service {
    function get() returns UnknownType
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    assert!(result.unwrap_err()[0].message.contains("Unknown type 'UnknownType'"));
}

#[test]
fn test_constant_named_type_error() {
    let code = r#"
const USER: User = "invalid" 
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    // Constants must be primitives (for now)
    assert!(result.is_err());
    assert!(result.unwrap_err()[0].message.contains("only primitives allowed"));
}

#[test]
fn test_array_base_type_validation() {
    let code = r#"
struct List {
    items: MissingType[]
    grid: MissingType[][]
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    // Should fail for MissingType (once or twice depending on how deep we check)
    assert!(errors.iter().any(|e| e.message.contains("Unknown type 'MissingType'")));
}

#[test]
fn test_struct_cycle_error() {
    let code = r#"
struct NodeA {
    b: NodeB
}

struct NodeB {
    a: NodeA
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors[0].message.contains("Cycle detected"));
}
