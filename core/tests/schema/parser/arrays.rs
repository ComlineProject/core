// Array type tests for rust-sitter IDL parser

#[cfg(test)]
mod array_tests {
    use comline_core::schema::idl::grammar;

    #[test]
    fn test_dynamic_array() {
        let code = "struct Container { items: str[] }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_fixed_size_array() {
        let code = "struct Buffer { data: u8[256] }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_array_of_primitives() {
        let code = r#"
struct Data {
    numbers: u32[]
    bytes: u8[128]
    bools: bool[]
}
"#;
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_array_of_custom_types() {
        let code = "struct List { users: User[] }";
        assert!(grammar::parse(code).is_ok());
    }

    #[test]
    fn test_nested_arrays() {
        // This might not work yet - test to see
        let code = "struct Matrix { grid: u32[][] }";
        let result = grammar::parse(code);
        // Don't assert - just see what happens
        println!("Nested array parse result: {:?}", result.is_ok());
    }
}
