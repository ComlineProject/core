// TEMPORARILY disabled - these tests use the old pest/lalrpop parser  
// TODO: Migrate to rust-sitter parser

/*
// Original tests - need migration to rust-sitter
#[test]
fn from_raw_to_unit() {
    // ... uses idl::parser::pest::parser_new
}

#[test]
fn compile_unit() {
    // ... uses idl::parser::pest::parser_new
}
*/

// TODO: Add new rust-sitter based tests here
// Example:
// #[test]
// fn parse_simple_struct() {
//     let code = "struct Test { field: str }";  
//     match comline_core::schema::idl::grammar::parse(code) {
//         Ok(_) => assert!(true),
//         Err(e) => panic!("Parse error: {:?}", e),
//     }
// }
