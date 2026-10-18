// Integration tests - complex real-world IDL examples

#[cfg(test)]
mod integration_tests {
    use comline_core::schema::idl::grammar;

    #[test]
    fn test_complete_api_schema() {
        let code = r#"
import std

const MAX_USERS: u32 = 1000
const API_VERSION: string = "v1"

enum Status {
    Active
    Inactive
    Pending
}

struct User {
    id: u64
    name: string
    email: string
    status: Status
}

struct UserList {
    users: User[]
    total: u32
}

protocol UserService {
    function createUser(string, string) -> u64;
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
    key: string
    value: string
}

struct Document {
    id: u64
    title: string
    metadata: Metadata[]
    tags: string[]
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
    function login(string, string) -> string;
    function logout(string) -> bool;
}

protocol DataService {
    function query(string) -> string;
    function update(u64, string) -> bool;
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
    strings: string[]
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
    sender: string
    recipient: string
    type: MessageType
    content: string
    timestamp: u64
}

struct Conversation {
    id: u64
    participants: string[]
    messages: Message[]
}

protocol MessagingService {
    function sendMessage(string, string, string) -> u64;
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
const MAX_S8: s8 = 127
const MIN_S8: s8 = -128
const ENABLED: bool = true
const NAME: string = "test"
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_function_variations() {
        let code = r#"
protocol TestService {
    function noArgs() -> string;
    function oneArg(u64) -> bool;
    function twoArgs(string, u32) -> string;
    function manyArgs(u8, u16, u32, u64, string, bool) -> s64;
    function noReturn(string);
    function arrayArg(string[]) -> u32;
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
    field: string // inline field comment
}

// Protocol comment  
protocol Service { // inline protocol comment
    // Function comment
    function test(u64) -> string; // inline function comment
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_whitespace_tolerance() {
        let code = "struct   Test   {   field  :  string   }";
        assert!(grammar::parse(code).is_ok());

        let code2 = "struct\tTest\t{\tfield:\tstr\t}";
        assert!(grammar::parse(code2).is_ok());

        let code3 = "\n\n\nstruct Test {\n\n\nfield: string\n\n\n}\n\n\n";
        assert!(grammar::parse(code3).is_ok());
    }

    #[test]
    fn test_settings_block() {
        // bool / int / string values, an empty block, and one alongside other
        // declarations.
        assert!(grammar::parse("settings S { a = true b = false }").is_ok());
        assert!(grammar::parse("settings S { n = 42 s = \"x\" }").is_ok());
        assert!(grammar::parse("settings Empty {}").is_ok());

        let code = r#"
/// switches
settings Project {
    forbid_indexing = true
    max_depth = 8
}

struct Message { body: string }
"#;
        assert!(grammar::parse(code).is_ok());

        // `true` / `false` stay valid identifiers elsewhere - the keyword only
        // applies inside a settings value.
        assert!(grammar::parse("enum E { true false }").is_ok());
    }

    #[test]
    fn test_unnamed_settings_block() {
        // No name after `settings` - applies implicitly to the whole file.
        assert!(grammar::parse("settings { a = true }").is_ok());
        assert!(grammar::parse("settings {}").is_ok());
    }

    #[test]
    fn test_dotted_settings_key() {
        assert!(
            grammar::parse("settings Foo { struct.field.validators.allowed = true }").is_ok()
        );
    }

    #[test]
    fn test_quoted_settings_key_segment() {
        // A diagnostic code id (`validation::deprecated-type-usage`) has
        // `::`/`-`, which no bare identifier allows - quoting that one
        // segment is the escape hatch, mixed freely with bare segments on
        // either side.
        assert!(grammar::parse(
            "settings Foo { diagnostics.\"validation::deprecated-type-usage\".level = \"warn\" }"
        )
        .is_ok());
        // A fully-quoted first segment too, for symmetry.
        assert!(grammar::parse("settings Foo { \"a::b-c\".level = \"error\" }").is_ok());
    }

    #[test]
    fn test_mode_bare_keyword() {
        // No quotes - a bare keyword, like any other `Identifier` value.
        assert!(grammar::parse("settings Foo { mode = replace }").is_ok());
    }

    #[test]
    fn test_settings_annotation_on_every_scope() {
        assert!(grammar::parse("settings {}\n@settings = Foo\nstruct S { x: string }").is_ok());
        assert!(grammar::parse("settings {}\nstruct S { @settings = Foo x: string }").is_ok());
        assert!(
            grammar::parse("settings {}\n@settings = Foo\nprotocol P { function f() -> string; }")
                .is_ok()
        );
        assert!(
            grammar::parse("settings {}\nprotocol P { @settings = Foo function f() -> string; }")
                .is_ok()
        );
        assert!(
            grammar::parse("settings {}\n@settings = Foo\nerror E { message = \"e\" }").is_ok()
        );
    }

    #[test]
    fn test_validator_declaration() {
        assert!(grammar::parse("validator V for string {}").is_ok());
        assert!(grammar::parse("validator V for string { a: u32 b: string = \"x\" }").is_ok());

        let code = r#"
/// bounds
validator StringBounds for string {
    min_chars: u32 = 0
    max_chars: u32 = 1024
}

struct Message { body: string }
"#;
        assert!(grammar::parse(code).is_ok());

        // `validator` is a keyword only in declaration position.
        assert!(grammar::parse("enum E { validator }").is_ok());
    }

    #[test]
    fn test_type_is_a_keyword_only_in_declaration_position() {
        // `type` is a far more common English word than `validator` - worth
        // pinning down explicitly as both an enum variant AND a field name,
        // not just one of the two.
        assert!(grammar::parse("enum E { type }").is_ok());
        assert!(grammar::parse("struct S { type: u32 }").is_ok());
    }

    #[test]
    fn test_validate_block() {
        assert!(grammar::parse(
            r#"validator V for string { x: u32 validate { assert(value.n >= params.x, "small") } }"#
        )
        .is_ok());

        // and / or chain, string operand, multiple asserts
        let code = r#"
validator StringBounds for string {
    min: u32 = 0
    validate {
        assert(value.length > params.max or value.length < params.min, "{value.name} out of range")
        assert(value.kind == "email", "must be an email")
    }
}
"#;
        assert!(grammar::parse(code).is_ok());

        // `assert` / `validate` / `and` / `or` are keywords only inside a
        // validator - still fine as identifiers elsewhere.
        assert!(grammar::parse("enum E { assert validate and or }").is_ok());
    }

    #[test]
    fn test_annotation_list_value() {
        // scalar annotations still parse
        assert!(grammar::parse("@timeout_ms = 1000\nstruct S { x: u32 }").is_ok());

        // list of calls with keyword args, on a field
        let code = r#"
struct Message {
    @validators = [StringBounds(min_chars = 3, max_chars = 12)]
    recipient: string
}
"#;
        assert!(grammar::parse(code).is_ok());

        // multiple calls, a call with no args, an empty list
        assert!(grammar::parse(
            "struct S {\n@validators = [A(x = 1), B()]\nf: string\n}"
        )
        .is_ok());
        assert!(grammar::parse("struct S {\n@validators = []\nf: string\n}").is_ok());
    }

    #[test]
    fn test_scoped_path_values() {
        // `::`-separated paths as const / default values
        assert!(grammar::parse("const MIN_LEN: u32 = u32::MIN").is_ok());
        assert!(grammar::parse("struct S { name: string = pkg::mod::DEFAULT }").is_ok());
        assert!(grammar::parse("validator V for string { min: u32 = u32::MIN }").is_ok());
        assert!(grammar::parse("@retries = core::defaults::MAX\nstruct S { x: u32 }").is_ok());

        // bare identifiers and literals still work (no `::` -> not a path)
        assert!(grammar::parse("const A: u32 = SOME_CONST").is_ok());
        assert!(grammar::parse("const B: u32 = 5").is_ok());
    }

    #[test]
    fn test_fstring_values() {
        assert!(grammar::parse(r#"const NAME: string = f"flower power: {POWER}""#).is_ok());
        assert!(grammar::parse(r#"const X: string = f"a {{lit}} {x.y} z""#).is_ok());
        assert!(grammar::parse(r#"struct S { name: string = f"hi {n}" }"#).is_ok());

        // `f` is not a keyword - only `f"` is. Identifiers / members named `f`
        // still parse.
        assert!(grammar::parse("const A: u32 = f").is_ok());
        assert!(grammar::parse("struct S { f: string }").is_ok());
        assert!(grammar::parse("protocol P { function f() -> string; }").is_ok());

        // plain strings stay non-interpolating
        assert!(grammar::parse(r#"const S: string = "plain {not_a_path}""#).is_ok());
    }

    #[test]
    fn comprehensive_fixture_parses() {
        // tests/schema/simple.ids touches import, const, settings, enum,
        // struct (defaults, @validators), error, and an annotated protocol.
        let src = std::fs::read_to_string("tests/schema/simple.ids")
            .expect("read tests/schema/simple.ids");
        let doc = grammar::parse(&src).expect("simple.ids should parse");
        assert_eq!(doc.0.len(), 9);
    }
}
