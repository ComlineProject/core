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
fn test_unknown_type_error_suggests_close_match() {
    let code = r#"
struct User {
    id: u64
}

struct Post {
    author: Uesr
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors[0].message.contains("Unknown type 'Uesr'"));
    assert!(errors[0].message.contains("did you mean 'User'?"));
}

#[test]
fn test_unknown_type_error_suggests_close_primitive() {
    let code = r#"
struct Foo {
    flag: boool
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors[0].message.contains("Unknown type 'boool'"));
    assert!(errors[0].message.contains("did you mean 'bool'?"));
}

#[test]
fn test_unknown_type_error_no_suggestion_when_nothing_close() {
    let code = r#"
struct User {
    id: u64
}

struct Foo {
    x: Zzzzzzzzzz
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors[0].message.contains("Unknown type 'Zzzzzzzzzz'"));
    assert!(!errors[0].message.contains("did you mean"));
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
    function get(UnknownType) -> bool;
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    assert!(result.unwrap_err()[0]
        .message
        .contains("Unknown type 'UnknownType'"));
}

#[test]
fn test_protocol_unknown_return_type() {
    let code = r#"
protocol Service {
    function get() -> UnknownType;
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    assert!(result.unwrap_err()[0]
        .message
        .contains("Unknown type 'UnknownType'"));
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
    assert!(result.unwrap_err()[0]
        .message
        .contains("only primitives allowed"));
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
    assert!(errors
        .iter()
        .any(|e| e.message.contains("Unknown type 'MissingType'")));
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

#[test]
fn test_duplicate_definition_error_has_span() {
    let code = "struct User {
    id: u64
}

struct User {
    name: str
}
";
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    let span = errors[0].span.expect("duplicate-definition error should carry a span");
    // The span should point at the second (duplicate) `struct User` declaration.
    assert_eq!(&code[span.0..span.1], "struct User {
    name: str
}");
}

#[test]
fn test_unknown_type_error_has_span() {
    let code = "struct Post {
    author: Author
}
";
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();

    let span = errors[0].span.expect("unknown-type error should carry a span");
    // Field-level granularity: the span covers the whole `author: Author` field.
    assert_eq!(&code[span.0..span.1], "author: Author");
}

#[test]
fn test_union_field_with_valid_members_passes() {
    let code = "struct Response {
    status: union(str u32)
}
";
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_ok(), "Expected valid union to pass validation, got {:?}", result.err());
}

#[test]
fn test_union_field_with_unknown_member_fails() {
    let code = "struct Response {
    status: union(str MissingType)
}
";
    let ir = IncrementalInterpreter::from_source(code);
    let result = validate(&ir);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Unknown type 'MissingType'")));
}

#[test]
fn test_field_validator_reference_resolves() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
}

struct Message {
    @validators = [StringBounds(min_chars = 3)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "declared validator should resolve: {:?}", validate(&ir).err());
}

#[test]
fn test_field_validator_reference_unknown_fails() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
}

struct Message {
    @validators = [StringBonds(min_chars = 3)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Unknown validator 'StringBonds'"), "got: {}", errors[0].message);
    assert!(errors[0].message.contains("did you mean 'StringBounds'?"));
}

#[test]
fn test_validator_reference_on_error_field() {
    let code = r#"
struct Message { body: str }

error NotFound {
    message = "{self.name} missing"

    @validators = [Missing()]
    name: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Unknown validator 'Missing'")));
}

#[test]
fn test_validator_reference_to_a_non_validator_fails() {
    let code = r#"
struct Helper { x: u32 }

struct Message {
    @validators = [Helper()]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("'Helper' is not a validator")), "got: {:?}", errors);
}

#[test]
fn test_imported_validator_reference_is_not_flagged() {
    // an imported symbol might be a validator - can't tell without the other
    // schema, so it must not error.
    let code = r#"
import std::validators::string_bounds::StringBounds

struct Message {
    @validators = [StringBounds(min_chars = 3)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "imported validator ref should not error: {:?}", validate(&ir).err());
}

#[test]
fn test_validator_kwargs_ok() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
    max_chars: u32 = 1024
}

struct Message {
    @validators = [StringBounds(min_chars = 3, max_chars = 12)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir).err());
}

#[test]
fn test_validator_unknown_kwarg_fails_with_suggestion() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
    max_chars: u32 = 1024
}

struct Message {
    @validators = [StringBounds(min_char = 3)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("validator 'StringBounds' has no argument 'min_char'"), "got: {}", errors[0].message);
    assert!(errors[0].message.contains("did you mean 'min_chars'?"));
}

#[test]
fn test_validator_duplicate_kwarg_fails() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
}

struct Message {
    @validators = [StringBounds(min_chars = 3, min_chars = 5)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("duplicate argument 'min_chars'")), "got: {:?}", errors);
}

#[test]
fn test_imported_validator_kwargs_not_checked() {
    // the other schema isn't loaded, so its properties are unknown - a bogus
    // kwarg on an imported validator can't be flagged here.
    let code = r#"
import std::validators::string_bounds::StringBounds

struct Message {
    @validators = [StringBounds(nonsense = 3)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir).err());
}

#[test]
fn test_validate_block_params_resolve() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
    validate {
        assert(value.length >= params.min_chars, "{value.name} too short")
    }
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir).err());
}

#[test]
fn test_validate_block_unknown_param_fails() {
    let code = r#"
validator StringBounds {
    min_chars: u32 = 0
    validate {
        assert(value.length >= params.min_char, "{value.name} too short")
    }
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e|
        e.message.contains("unknown property 'params.min_char'")
        && e.message.contains("did you mean 'params.min_chars'?")
    ), "got: {:?}", errors);
}

#[test]
fn test_validate_block_bad_root_fails() {
    let code = r#"
validator V {
    n: u32 = 0
    validate {
        assert(field.length >= params.n, "bad")
    }
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e|
        e.message.contains("`field` is not a valid reference in `validate`")
    ), "got: {:?}", errors);
}

#[test]
fn test_validator_kwarg_type_ok() {
    let code = r#"
validator Bounds {
    min: u32 = 0
    strict: bool = false
    label: str = "x"
}

struct Message {
    @validators = [Bounds(min = 3, strict = true, label = "body")]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir).err());
}

#[test]
fn test_validator_kwarg_string_for_int_fails() {
    let code = r#"
validator Bounds { min: u32 = 0 }

struct Message {
    @validators = [Bounds(min = "three")]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("argument 'min' expects integer, got string")), "got: {:?}", errors);
}

#[test]
fn test_validator_kwarg_int_for_bool_fails() {
    let code = r#"
validator Flag { on: bool = false }

struct Message {
    @validators = [Flag(on = 1)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    let errors = validate(&ir).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("argument 'on' expects bool, got integer")), "got: {:?}", errors);
}

#[test]
fn test_validator_kwarg_reference_value_not_type_checked() {
    // a bare identifier value is unresolved - can't judge its type.
    let code = r#"
const MIN: u32 = 3
validator Bounds { min: u32 = 0 }

struct Message {
    @validators = [Bounds(min = MIN)]
    body: str
}
"#;
    let ir = IncrementalInterpreter::from_source(code);
    assert!(validate(&ir).is_ok(), "{:?}", validate(&ir).err());
}
