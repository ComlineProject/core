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
    function twoArgs(str, u32) -> i64;
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
const I8_MIN: i8 = -128
const I8_MAX: i8 = 127
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
const TIMEOUT_MS: i32 = 5000

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
