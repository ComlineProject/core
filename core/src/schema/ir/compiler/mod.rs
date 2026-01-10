// Relative Modules
pub mod interpreter;
pub mod interpreted;
pub mod report;

// Standard Uses

// Local Uses
use crate::schema::idl::grammar::Declaration;
// use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc};

// External Uses



pub trait Compile {
    type Output;

    /// Compile from rust-sitter AST (new approach)
    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output;
    
    fn from_source(source: &str) -> Self::Output {
        tracing::debug!("Compiling source with rust-sitter...");
        
        // Parse with rust-sitter grammar (returns Document with Vec<Declaration>)
        match crate::schema::idl::grammar::parse(source) {
            Ok(document) => {
                // Extract declarations from Document (field 0 is Vec<Declaration>)
                let declarations = document.0;
                Self::from_declarations(declarations)
            }
            Err(e) => {
                panic!("Parse error: {:?}", e);
            }
        }
    }
}
