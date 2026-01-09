// Relative Modules
pub mod interpreter;
pub mod interpreted;
pub mod report;

// Standard Uses

// Local Uses
use crate::schema::idl::grammar::Declaration;
use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc};

// External Uses



pub trait Compile {
    type Output;

    /// Compile from rust-sitter AST (new approach)
    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output;
    
    /// Legacy method - compile from old ASTUnit format
    /// TODO: Remove once migration to Declaration is complete
    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output;

    fn from_source(source: &str) -> Self::Output {
        println!("Compiling source with rust-sitter...");
        
        // Parse with rust-sitter grammar
        match crate::schema::idl::grammar::parse(source) {
            Ok(declaration) => {
                // Wrap single declaration in Vec for unified interface
                // TODO: Update grammar to parse multiple declarations (Vec<Declaration>)
                Self::from_declarations(vec![declaration])
            }
            Err(e) => {
                panic!("Parse error: {:?}", e);
            }
        }
    }

    /// Legacy method for old parser integration
    /// TODO: Remove once migration complete
    fn from_sourced_whole(sourced: SourcedWholeRc) -> Self::Output;
}
