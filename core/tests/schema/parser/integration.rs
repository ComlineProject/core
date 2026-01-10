// Integration tests - complex real-world IDL examples

#[cfg(test)]
mod integration_tests {
    use comline_core::schema::idl::grammar;

    #[test]
    fn test_complete_api_schema() {
        let code = r#"
import std

const MAX_USERS: u32 = 1000
const API_VERSION: str = "v1"

enum Status {
    Active
    Inactive
    Pending
}

struct User {
    id: u64
    name: str
    email: str
    status: Status
}

struct UserList {
    users: User[]
    total: u32
}

protocol UserService {
    function createUser(str, str) -> u64;
    function getUser(u64) -> User;
    function listUsers() -> UserList;
    function deleteUser(u64) -> bool;
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_complex_nested_structures() {
        let code = r#"
struct Metadata {
    key: str
    value: str
}

struct Document {
    id: u64
    title: str
    metadata: Metadata[]
    tags: str[]
}

struct Collection {
    documents: Document[]
    count: u32
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_multiple_protocols() {
        let code = r#"
protocol AuthService {
    function login(str, str) -> str;
    function logout(str) -> bool;
}

protocol DataService {
    function query(str) -> str;
    function update(u64, str) -> bool;
}

protocol AdminService {
    function reset() -> bool;
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_mixed_array_types() {
        let code = r#"
struct ComplexData {
    integers: u32[]
    fixedBytes: u8[256]
    strings: str[]
    ids: u64[10]
    flags: bool[]
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_real_world_message_protocol() {
        let code = r#"
enum MessageType {
    Text
    Image
    Video
}

struct Message {
    id: u64
    sender: str
    recipient: str
    type: MessageType
    content: str
    timestamp: u64
}

struct Conversation {
    id: u64
    participants: str[]
    messages: Message[]
}

protocol MessagingService {
    function sendMessage(str, str, str) -> u64;
    function getConversation(u64) -> Conversation;
    function markAsRead(u64) -> bool;
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_constants_all_types() {
        let code = r#"
const MAX_U8: u8 = 255
const MAX_U16: u16 = 65535
const MAX_U32: u32 = 4294967295
const MAX_I8: i8 = 127
const MIN_I8: i8 = -128
const ENABLED: bool = true
const NAME: str = "test"
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_function_variations() {
        let code = r#"
protocol TestService {
    function noArgs() -> str;
    function oneArg(u64) -> bool;
    function twoArgs(str, u32) -> str;
    function manyArgs(u8, u16, u32, u64, str, bool) -> i64;
    function noReturn(str);
    function arrayArg(str[]) -> u32;
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_enum_variations() {
        let code = r#"
enum SingleVariant {
    One
}

enum TwoVariants {
    First
    Second
}

enum ManyVariants {
    Alpha
    Beta
    Gamma
    Delta
    Epsilon
    Zeta
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_comments_everywhere() {
        let code = r#"
// File header comment
import std // inline import comment

// Constant comment
const VALUE: u32 = 100 // inline value comment

// Struct comment
struct Data { // inline struct comment
    // Field comment
    field: str // inline field comment
}

// Protocol comment  
protocol Service { // inline protocol comment
    // Function comment
    function test(u64) -> str; // inline function comment
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_whitespace_tolerance() {
        let code = "struct   Test   {   field  :  str   }";
        assert!(grammar::parse(code).is_ok());

        let code2 = "struct\tTest\t{\tfield:\tstr\t}";
        assert!(grammar::parse(code2).is_ok());

        let code3 = "\n\n\nstruct Test {\n\n\nfield: str\n\n\n}\n\n\n";
        assert!(grammar::parse(code3).is_ok());
    }
}
