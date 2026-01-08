use lalrpop_util::lalrpop_mod;

lalrpop_mod!(pub idl_parser, "/schema/idl/parser/lalrpop/idl.rs");



#[test]
fn calculator1() {
    let s: Loc =
    assert!(idl_parser::TermParser::new().parse("22").is_ok());
    assert!(idl_parser::TermParser::new().parse("(22)").is_ok());
    assert!(idl_parser::TermParser::new().parse("((((22))))").is_ok());
    assert!(idl_parser::TermParser::new().parse("((22)").is_err());
}