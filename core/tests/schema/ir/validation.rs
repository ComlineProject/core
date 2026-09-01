// IR validation tests - verify actual FrozenUnit content

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;

#[cfg(test)]
mod ir_validation_tests {
    use super::*;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;

    // These tests would ideally validate the actual IR content,
    // but since from_declarations -> (), we verify no panics occur

    #[test]
    fn test_struct_field_types() {
        let code = r#"
struct TestStruct {
    id: u64
    name: str
    count: u32
    active: bool
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        // Verify IR generation content
        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "TestStruct");
                assert_eq!(fields.len(), 4);
                // We could check individual field types here
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_struct_and_field_docstrings_populate_ir() {
        let code = r#"
/// A user account.
struct User {
    /// The user's unique id.
    id: u64
    name: str
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct {
                docstring,
                fields,
                ..
            } => {
                assert_eq!(docstring.as_deref(), Some("A user account."));

                match &fields[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        docstring, ..
                    } => {
                        assert_eq!(docstring.as_deref(), Some("The user's unique id."));
                    }
                    _ => panic!("Expected Field"),
                }
                // A field with no docstring keeps behaving exactly as
                // before the shared build_kind_value/docstring wiring.
                match &fields[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        docstring, ..
                    } => {
                        assert_eq!(*docstring, None);
                    }
                    _ => panic!("Expected Field"),
                }
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_multi_line_docstring_is_newline_joined() {
        let code = r#"
/// Checks if a string length is within bounds.
/// @min_chars: Minimum length of the string.
struct StringBounds {
    min_chars: u32
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { docstring, .. } => {
                assert_eq!(
                    docstring.as_deref(),
                    Some(
                        "Checks if a string length is within bounds.\n@min_chars: Minimum length of the string."
                    )
                );
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_protocol_and_function_docstrings_populate_ir() {
        let code = r#"
/// A user service.
protocol UserService {
    /// Get a user by id.
    function get(u64) -> str;
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol {
                docstring,
                functions,
                ..
            } => {
                assert_eq!(docstring, "A user service.");

                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        docstring, ..
                    } => {
                        assert_eq!(docstring, "Get a user by id.");
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_enum_and_const_docstrings_populate_ir() {
        let code = r#"
/// Status values.
enum Status {
    Active
}

/// The max allowed.
const MAX: u32 = 10
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Enum { docstring, .. } => {
                assert_eq!(docstring.as_deref(), Some("Status values."));
            }
            _ => panic!("Expected Enum"),
        }
        match &ir_units[1] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Constant { docstring, .. } => {
                assert_eq!(docstring.as_deref(), Some("The max allowed."));
            }
            _ => panic!("Expected Constant"),
        }
    }

    #[test]
    fn test_error_declaration_populates_ir() {
        let code = r#"
/// Thrown when a user can't be found.
error NotFoundError {
    message = "{self.id} was not found"
    id: u64
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Error {
                docstring,
                name,
                message,
                fields,
                ..
            } => {
                assert_eq!(
                    docstring.as_deref(),
                    Some("Thrown when a user can't be found.")
                );
                assert_eq!(name, "NotFoundError");
                // The placeholder round-trips back to its original,
                // un-escaped single-brace form.
                assert_eq!(message, "{self.id} was not found");
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_error_message_escaped_braces_round_trip() {
        let code = "error E {\n    message = \"code: {{404}}\"\n}";
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Error { message, .. } => {
                assert_eq!(message, "code: {404}");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_function_throws_populates_ir() {
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        // `NotFoundError` is neither declared locally nor imported: it still
        // gets a stable ordinal (0), and an `<unresolved: ...>` re-export slot
        // is appended so the ordinal has a home in the IR.
        let code = "protocol P {\n    function get(u64) -> str ! NotFoundError;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);

        let FrozenUnit::Protocol { functions, .. } = &ir_units[0] else {
            panic!("Expected Protocol");
        };
        let FrozenUnit::Function { throws, .. } = &functions[0] else {
            panic!("Expected Function");
        };
        assert_eq!(throws, &vec![0u16]);

        let reexport = ir_units
            .iter()
            .find_map(|u| match u {
                FrozenUnit::Error { name, ordinal, imported_from, .. } if name == "NotFoundError" => {
                    Some((*ordinal, imported_from.clone()))
                }
                _ => None,
            })
            .expect("an Error unit for the thrown name");
        assert_eq!(reexport.0, 0);
        assert_eq!(reexport.1.as_deref(), Some("<unresolved: NotFoundError>"));
    }

    #[test]
    fn test_function_without_throws_still_empty_vec() {
        // A function with no throws clause still gets an empty Vec (now
        // Vec<u16>, resolved from the `! Name` references).
        let code = "protocol P {\n    function get(u64) -> str;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        throws,
                        ..
                    } => {
                        assert!(throws.is_empty());
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_local_error_gets_ordinal_and_throws_resolves_to_it() {
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let code = "\
error Missing {\n    message = \"gone\"\n}\n\
error Denied {\n    message = \"no\"\n}\n\
protocol P {\n    function get(u64) -> str ! Denied;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);

        // Declaration order: Missing = 0, Denied = 1.
        let ordinal_of = |want: &str| {
            ir_units.iter().find_map(|u| match u {
                FrozenUnit::Error { name, ordinal, imported_from: None, .. } if name == want => {
                    Some(*ordinal)
                }
                _ => None,
            })
        };
        assert_eq!(ordinal_of("Missing"), Some(0));
        assert_eq!(ordinal_of("Denied"), Some(1));

        let FrozenUnit::Protocol { functions, .. } =
            ir_units.iter().find(|u| matches!(u, FrozenUnit::Protocol { .. })).unwrap()
        else {
            unreachable!()
        };
        let FrozenUnit::Function { throws, .. } = &functions[0] else {
            panic!("Expected Function");
        };
        assert_eq!(throws, &vec![1u16]);
    }

    #[test]
    fn test_struct_field_default_values() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};

        let code = r#"
struct Config {
    retries: u32 = 3
    label: str = "default"
    timeout: s64 = -1
    name: str
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { fields, .. } => {
                assert_eq!(fields.len(), 4);

                match &fields[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::U64(Some(3)))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                match &fields[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::String(Some("default".to_string())))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                match &fields[2] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::S64(Some(-1)))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                // A field with no default keeps behaving exactly as before
                // the shared build_kind_value refactor.
                match &fields[3] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Namespaced("str".to_string(), None)
                        );
                    }
                    _ => panic!("Expected Field"),
                }
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_enum_variants() {
        let code = r#"
enum Color {
    Red
    Green
    Blue
    Yellow
    Black
    White
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Enum { name, .. } => {
                assert_eq!(name, "Color");
                // TODO: Verify variants count when exposed
            }
            _ => panic!("Expected Enum"),
        }
    }

    #[test]
    fn test_function_arguments_mapping() {
        let code = r#"
protocol TestService {
    function noArgs() -> str;
    function oneArg(u64) -> bool;
    function twoArgs(str, u32) -> s64;
    function manyArgs(u8, u16, u32, u64, str, bool) -> str;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol {
                name,
                functions,
                ..
            } => {
                assert_eq!(name, "TestService");
                assert_eq!(functions.len(), 4);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_function_argument_names() {
        let code = r#"
protocol TestService {
    function bareArgs(u64, str) -> bool;
    function namedArgs(id: u64, name: str) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        arguments, ..
                    } => {
                        // No names given - falls back to synthesized arg0/arg1.
                        assert_eq!(arguments[0].name, "arg0");
                        assert_eq!(arguments[1].name, "arg1");
                    }
                    _ => panic!("Expected Function"),
                }
                match &functions[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        arguments, ..
                    } => {
                        // Real names given - used as-is.
                        assert_eq!(arguments[0].name, "id");
                        assert_eq!(arguments[1].name, "name");
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_function_return_types() {
        let code = r#"
protocol ReturnTypes {
    function getU64() -> u64;
    function getStr() -> str;
    function getBool() -> bool;
    function getArray() -> str[];
    function noReturn(u64);
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                assert_eq!(functions.len(), 5);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_const_primitive_values() {
        let code = r#"
const U8_VAL: u8 = 255
const U16_VAL: u16 = 65535
const U32_VAL: u32 = 4294967295
const I8_MIN: s8 = -128
const I8_MAX: s8 = 127
const BOOL_TRUE: bool = true
const BOOL_FALSE: bool = false
const STR_VAL: str = "hello"
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 8); // 8 constants
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Constant { name, .. } => {
                assert_eq!(name, "U8_VAL");
            }
            _ => panic!("Expected Constant"),
        }
    }

    #[test]
    fn test_nested_custom_types() {
        let code = r#"
struct Inner {
    value: u64
}

struct Outer {
    inner: Inner
    items: Inner[]
}

protocol Service {
    function get() -> Outer;
    function process(Outer) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 3); // inner, outer, service
        match &ir_units[1] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "Outer");
                assert_eq!(fields.len(), 2);
            }
            _ => panic!("Expected Outer Struct"),
        }
    }

    #[test]
    fn test_mixed_array_types() {
        let code = r#"
struct ArrayTest {
    dynamic: str[]
    fixed: u8[256]
    nested: u32[][]
    custom_array: Inner[]
    fixed_custom: Inner[10]
}

struct Inner {
    id: u64
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 2); // Struct + Inner Struct
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "ArrayTest");
                assert_eq!(fields.len(), 5);
            }
            _ => panic!("Expected ArrayTest Struct"),
        }
    }

    #[test]
    fn test_all_declaration_types_together() {
        let code = r#"
import std

const VERSION: str = "1.0"
const MAX: u32 = 1000

enum Status {
    Active
    Inactive
}

struct Data {
    id: u64
    status: Status
}

protocol API {
    function get(u64) -> Data;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        // import + 2 consts + enum + struct + protocol = 6 units
        assert_eq!(ir_units.len(), 6);
    }

    #[test]
    fn test_complex_real_world_schema() {
        let code = r#"
import std

const API_VERSION: str = "2.0"
const MAX_USERS: u32 = 10000
const TIMEOUT_MS: s32 = 5000

enum UserRole {
    Admin
    User
    Guest
}

enum MessageType {
    Text
    Image
    Video
    File
}

struct Address {
    street: str
    city: str
    country: str
}

struct User {
    id: u64
    username: str
    email: str
    role: UserRole
    address: Address
    tags: str[]
}

struct Message {
    id: u64
    sender_id: u64
    type: MessageType
    content: str
    timestamp: u64
}

struct Conversation {
    id: u64
    participants: u64[]
    messages: Message[]
}

protocol UserService {
    function createUser(str, str, UserRole) -> u64;
    function getUser(u64) -> User;
    function updateUser(u64, str) -> bool;
    function deleteUser(u64) -> bool;
    function listUsers(u32, u32) -> User[];
}

protocol MessagingService {
    function sendMessage(u64, u64, MessageType, str) -> u64;
    function getConversation(u64) -> Conversation;
    function markAsRead(u64) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        // import + 3 consts + 2 enums + 4 structs + 2 protocols = 12 units
        assert_eq!(ir_units.len(), 12);
    }
}
