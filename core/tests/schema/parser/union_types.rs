// Union type tests for rust-sitter IDL parser

#[cfg(test)]
mod union_tests {
    use comline_core::schema::idl::grammar;

    #[test]
    fn test_union_field() {
        let code = "struct Response { status: union(str u32) }";
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse union field: {:?}", result.err());
    }

    #[test]
    fn test_union_three_members() {
        let code = "struct Response { status: union(str u32 bool) }";
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse three-member union: {:?}", result.err());
    }

    #[test]
    fn test_union_of_named_types() {
        let code = r#"
struct Ok { value: str }
struct Err { message: str }
struct Result { outcome: union(Ok Err) }
"#;
        let result = grammar::parse(code);
        assert!(result.is_ok(), "Failed to parse union of named types: {:?}", result.err());
    }

    #[test]
    fn test_union_requires_at_least_one_member() {
        let code = "struct Response { status: union() }";
        let result = grammar::parse(code);
        assert!(result.is_err(), "Empty union should not parse");
    }
}
