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

        // Generate IR
        let ir = IncrementalInterpreter::from_source(code);

        assert!()
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

        let ir = IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_protocol_with_functions_ir() {
        let code = r#"
protocol UserService {
    function getUser(u64) returns str
    function createUser(str, str) returns u64
    function deleteUser(u64) returns bool
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse protocol");

        IncrementalInterpreter::from_source(code);
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

        IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_import_ir() {
        let code = "import std";
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse import");

        IncrementalInterpreter::from_source(code);
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

        IncrementalInterpreter::from_source(code);
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
    function get(u64) returns User
    function list() returns User[]
    function delete(u64) returns bool
}
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse complete IDL");

        IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_protocol_no_args_ir() {
        let code = r#"
protocol Service {
    function reset() returns bool
    function status() returns str
}
"#;
        let result = grammar::parse(code);
        assert!(
            result.is_ok(),
            "Failed to parse protocol with no-arg functions"
        );

        IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_protocol_no_return_ir() {
        let code = r#"
protocol EventService {
    function notify(str)
    function log(str, u32)
}
"#;
        let result = grammar::parse(code);
        assert!(
            result.is_ok(),
            "Failed to parse protocol with no-return functions"
        );

        IncrementalInterpreter::from_source(code);
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

        IncrementalInterpreter::from_source(code);
    }
}
