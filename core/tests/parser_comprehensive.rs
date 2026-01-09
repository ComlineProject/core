// Comprehensive test suite for rust-sitter IDL parser
// Covers edge cases, error handling, and all grammar features

#[cfg(test)]
mod parser_tests {
    use comline_core::schema::idl::grammar;

    // ===== STRUCT TESTS =====

    #[test]
    fn test_simple_struct() {
        let code = "struct User { name: str }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_struct_multiple_fields() {
        let code = r#"
struct Person {
    name: str
    age: u8
    email: str
    active: bool
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_struct_all_primitive_types() {
        let code = r#"
struct AllTypes {
    i8_field: i8
    i16_field: i16
    i32_field: i32
    i64_field: i64
    u8_field: u8
    u16_field: u16
    u32_field: u32
    u64_field: u64
    f32_field: f32
    f64_field: f64
    bool_field: bool
    str_field: str
    string_field: string
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_struct_with_custom_type() {
        let code = "struct Container { data: CustomType }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_empty_struct() {
        let code = "struct Empty { }";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== ENUM TESTS =====

    #[test]
    fn test_simple_enum() {
        let code = "enum Status { Active }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_enum_multiple_variants() {
        let code = r#"
enum Color {
    Red
    Green
    Blue
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_enum_many_variants() {
        let code = r#"
enum DayOfWeek {
    Monday
    Tuesday
    Wednesday
    Thursday
    Friday
    Saturday
    Sunday
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    // ===== PROTOCOL TESTS =====

    #[test]
    fn test_simple_protocol() {
        let code = "protocol API { function get() returns str }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_protocol_no_return() {
        let code = "protocol API { function notify(str) }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    #[ignore] // TODO: Requires comma-separated arguments (Phase B: Grammar Expansion)
    fn test_protocol_multiple_args() {
        let code = "protocol API { function process(str, u32, bool) returns i64 }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    #[ignore] // TODO: Requires comma-separated arguments (Phase B: Grammar Expansion)
    fn test_protocol_multiple_functions() {
        let code = r#"
protocol UserService {
    function create(str) returns u64
    function read(u64) returns str
    function update(u64, str) returns bool
    function delete(u64) returns bool
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_empty_protocol() {
        let code = "protocol Empty { }";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== CONST TESTS =====

    #[test]
    fn test_const_integer() {
        let code = "const MAX: u32 = 100";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_const_string() {
        let code = r#"const NAME: str = "hello""#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_const_identifier_value() {
        let code = "const DEFAULT: str = OTHER_CONST";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== IMPORT TESTS =====

    #[test]
    fn test_simple_import() {
        let code = "import std";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_import_complex_name() {
        let code = "import my_module_123";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== WHITESPACE TESTS =====

    #[test]
    fn test_extra_whitespace() {
        let code = "   struct   User   {   name  :  str   }   ";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_tabs() {
        let code = "struct\tUser\t{\tname:\tstr\t}";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_multiple_newlines() {
        let code = "\n\n\nstruct User {\n\n\nname: str\n\n\n}\n\n\n";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_mixed_whitespace() {
        let code = "\t  \n  struct User {\n\t  name: str\n  \t}\n  ";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== COMMENT TESTS =====

    #[test]
    fn test_comment_before_struct() {
        let code = r#"
// This is a comment
struct User { name: str }
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_comment_after_field() {
        let code = r#"
struct User {
    name: str  // user's name
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_multiple_comments() {
        let code = r#"
// Comment 1
// Comment 2
struct User { // inline comment
    name: str // field comment
} // end comment
"#;
        assert!(grammar::parse(code).is_ok());
    }

    // ===== IDENTIFIER TESTS =====

    #[test]
    fn test_identifier_with_numbers() {
        let code = "struct User123 { field456: str }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_identifier_with_underscores() {
        let code = "struct My_User_Type { my_field_name: str }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_identifier_starts_with_underscore() {
        let code = "struct _Private { _field: str }";
        assert!(grammar::parse(code).is_ok());
    }

    // ===== NEGATIVE TESTS (should fail) =====

    #[test]
    fn test_invalid_empty_input() {
        let code = "";
        assert!(grammar::parse(code).is_err());
    }

    #[test]
    fn test_invalid_just_whitespace() {
        let code = "   \n\t  \n  ";
        assert!(grammar::parse(code).is_err());
    }

    #[test]
    fn test_invalid_incomplete_struct() {
        let code = "struct User {";
        assert!(grammar::parse(code).is_err());
    }

    #[test]
    fn test_invalid_missing_field_type() {
        let code = "struct User { name }";
        assert!(grammar::parse(code).is_err());
    }

    #[test]
    fn test_invalid_identifier_starts_with_number() {
        let code = "struct 123User { field: str }";
        assert!(grammar::parse(code).is_err());
    }
}
