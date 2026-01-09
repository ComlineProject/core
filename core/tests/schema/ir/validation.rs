// IR validation tests - verify actual FrozenUnit content

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;

#[cfg(test)]
mod ir_validation_tests {
    use super::*;
    use comline_core::schema::ir::compiler::Compile;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;

    // These tests would ideally validate the actual IR content,
    // but since from_declarations returns (), we verify no panics occur
    
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
        
        // Verify IR generation doesn't panic
        IncrementalInterpreter::from_source(code);
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
        
        IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_function_arguments_mapping() {
        let code = r#"
protocol TestService {
    function noArgs() returns str
    function oneArg(u64) returns bool
    function twoArgs(str, u32) returns i64
    function manyArgs(u8, u16, u32, u64, str, bool) returns str
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());
        
        IncrementalInterpreter::from_source(code);
    }

    #[test]
    fn test_function_return_types() {
        let code = r#"
protocol ReturnTypes {
    function getU64() returns u64
    function getStr() returns str
    function getBool() returns bool
    function getArray() returns str[]
    function noReturn(u64)
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());
        
        IncrementalInterpreter::from_source(code);
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
        
        IncrementalInterpreter::from_source(code);
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
    function get() returns Outer
    function process(Outer) returns bool
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());
        
        IncrementalInterpreter::from_source(code);
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
        
        IncrementalInterpreter::from_source(code);
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
    function get(u64) returns Data
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());
        
        IncrementalInterpreter::from_source(code);
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
    function createUser(str, str, UserRole) returns u64
    function getUser(u64) returns User
    function updateUser(u64, str) returns bool
    function deleteUser(u64) returns bool
    function listUsers(u32, u32) returns User[]
}

protocol MessagingService {
    function sendMessage(u64, u64, MessageType, str) returns u64
    function getConversation(u64) returns Conversation
    function markAsRead(u64) returns bool
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());
        
        IncrementalInterpreter::from_source(code);
    }
}
