// Relative Modules
pub mod interpreter;
pub mod interpreted;
pub mod report;

// Standard Uses

// Local Uses
// TODO: Re-implement with rust-sitter parser
// use crate::schema::idl::parser::pest::parser_new;
use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc};

// External Uses



pub trait Compile {
    type Output;

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output;

    fn from_source(source: &str) -> Self::Output {
        println!("Compiling source: {}", source);
        // TODO: Re-implement with rust-sitter parser
        unimplemented!("from_source not yet implemented with rust-sitter")
        // let sourced = parser_new::parse_source(
        //     source.to_owned(), "TODO".to_owned()
        // ).unwrap();
        // Self::from_sourced_whole(sourced)
    }


    fn from_sourced_whole(sourced: SourcedWholeRc) -> Self::Output;
}
