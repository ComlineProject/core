// Comprehensive IR generation tests

use comline_core::schema::idl::grammar;

#[cfg(test)]
mod ir_generation_tests {
    use super::*;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;

    #[test]
    fn test_simple_struct_ir() {
        let code = r#"
struct User {
    id: u64
    name: str
}
"#;
        // Should not panic - basic smoke test
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse simple struct");

        // Generate IR and validate content
        let ir_units = IncrementalInterpreter::from_source(code);

        // Should generate 1 IR unit (the struct)
        assert_eq!(ir_units.len(), 1, "Expected 1 IR unit");

        // Validate it's a Struct with correct structure
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "User", "Struct name should be 'User'");
                assert_eq!(fields.len(), 2, "Should have 2 fields");
            }
            _ => panic!("Expected Struct IR unit, got {:?}", ir_units[0]),
        }
    }

    #[test]
    fn test_enum_ir() {
        let code = r#"
enum Status {
    Active
    Inactive
    Pending
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse enum");

        let ir_units = IncrementalInterpreter::from_source(code);

        assert_eq!(ir_units.len(), 1);

        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Enum { name, .. } => {
                assert_eq!(name, "Status");
                // Note: We'd check variants here, but currently just validating name
            }
            _ => panic!("Expected Enum unit"),
        }
    }

    #[test]
    fn test_protocol_with_functions_ir() {
        let code = r#"
protocol UserService {
    function getUser(u64) -> str;
    function createUser(str, str) -> u64;
    function deleteUser(u64) -> bool;
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse protocol");

        let ir_units = IncrementalInterpreter::from_source(code);

        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol {
                name,
                functions,
                ..
            } => {
                assert_eq!(name, "UserService");
                assert_eq!(functions.len(), 3);
            }
            _ => panic!("Expected Protocol unit"),
        }
    }

    #[test]
    fn test_const_ir() {
        let code = r#"
const MAX_USERS: u32 = 1000
const API_VERSION: str = "v1.0"
const ENABLED: bool = true
const MIN_VALUE: i8 = -128
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse constants");

        let ir_units = IncrementalInterpreter::from_source(code);

        assert_eq!(ir_units.len(), 4); // 4 constants

        // Just verify types
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Constant { name, .. } => {
                assert_eq!(name, "MAX_USERS");
            }
            _ => panic!("Expected Constant unit"),
        }
    }

    #[test]
    fn test_import_ir() {
        let code = "import std";
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse import");

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Import(path, _) => {
                assert_eq!(path, "std");
            }
            _ => panic!("Expected Import unit"),
        }
    }

    #[test]
    fn test_struct_with_arrays_ir() {
        let code = r#"
struct Container {
    items: str[]
    buffer: u8[256]
    matrix: u32[][]
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse struct with arrays");

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "Container");
                assert_eq!(fields.len(), 3);
            }
            _ => panic!("Expected Struct unit"),
        }
    }

    #[test]
    fn test_complete_idl_ir() {
        let code = r#"
import std

const MAX_USERS: u32 = 100

enum Status {
    Active
    Inactive
}

struct User {
    id: u64
    name: str
    status: Status
    tags: str[]
}

protocol API {
    function get(u64) -> User;
    function list() -> User[];
    function delete(u64) -> bool;
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse complete IDL");

        let ir_units = IncrementalInterpreter::from_source(code);
        // Expecting: 1 import, 1 const, 1 enum, 1 struct, 1 protocol = 5 units
        assert_eq!(ir_units.len(), 5);
    }

    #[test]
    fn test_protocol_no_args_ir() {
        let code = r#"
protocol Service {
    function reset() -> bool;
    function status() -> str;
}
"#;
        let result = grammar::parse(code);
        assert!(
            result.is_ok(),
            "Failed to parse protocol with no-arg functions"
        );

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                assert_eq!(functions.len(), 2);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_protocol_no_return_ir() {
        let code = r#"
protocol EventService {
    function notify(str);
    function log(str, u32);
}
"#;
        let result = grammar::parse(code);
        assert!(
            result.is_ok(),
            "Failed to parse protocol with no-return functions"
        );

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                assert_eq!(functions.len(), 2);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_struct_and_field_spans_are_populated() {
        let code = "struct User {\n    id: u64\n    name: str\n}\n";

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);

        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { span, fields, .. } => {
                // The struct's span should cover the whole declaration.
                assert_eq!(&code[span.0..span.1], "struct User {\n    id: u64\n    name: str\n}");

                assert_eq!(fields.len(), 2);
                let field_spans: Vec<(usize, usize)> = fields
                    .iter()
                    .map(|f| match f {
                        comline_core::schema::ir::frozen::unit::FrozenUnit::Field { span, .. } => *span,
                        _ => panic!("Expected Field"),
                    })
                    .collect();

                assert_eq!(&code[field_spans[0].0..field_spans[0].1], "id: u64");
                assert_eq!(&code[field_spans[1].0..field_spans[1].1], "name: str");
                // Fields should appear in source order.
                assert!(field_spans[0].0 < field_spans[1].0);
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_union_field_ir() {
        let code = r#"
struct Response {
    status: union(str u32)
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse union field");

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);

        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { fields, .. } => {
                assert_eq!(fields.len(), 1);
                match &fields[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field { kind_value, .. } => {
                        match kind_value {
                            comline_core::schema::ir::compiler::interpreted::kind_search::KindValue::Union(members) => {
                                assert_eq!(members.len(), 2);
                            }
                            other => panic!("Expected KindValue::Union, got {:?}", other),
                        }
                    }
                    _ => panic!("Expected Field"),
                }
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_multiple_structs_ir() {
        let code = r#"
struct Address {
    street: str
    city: str
}

struct User {
    id: u64
    address: Address
}

struct Company {
    name: str
    employees: User[]
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse multiple structs");

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 3);
        match &ir_units[2] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, .. } => {
                assert_eq!(name, "Company");
            }
            _ => panic!("Expected Struct"),
        }
    }
}
