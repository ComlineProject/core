// Relative Modules
pub mod interpreter;
pub mod interpreted;
pub mod report;

// Standard Uses

// Local Uses
use crate::schema::idl::parser_new;
use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc};

// External Uses



pub trait Compile {
    type Output;

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output;

    fn from_source(source: &str) -> Self::Output {
        println!("Compiling source: {}", source);

        let sourced = parser_new::parse_source(
            source.to_owned(), "TODO".to_owned() // TODO: We need the source name here
        ).unwrap();

        Self::from_sourced_whole(sourced)
    }


    fn from_sourced_whole(sourced: SourcedWholeRc) -> Self::Output;
}
