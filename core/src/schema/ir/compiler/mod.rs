// Relative Modules
pub mod interpreter;
pub mod interpreted;
pub mod import_resolver;
pub mod alias_resolution;
pub mod annotation_forms;
pub mod bounds;
pub mod settings;

// Standard Uses

// Crate Uses
use crate::schema::idl::grammar::Declaration;
// use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc};

// External Uses



pub trait Compile {
    type Output;

    /// Compile from rust-sitter AST (new approach)
    fn from_declarations(declarations: Vec<rust_sitter::Spanned<Declaration>>) -> Self::Output;
    
    fn from_source(source: &str) -> Self::Output {
        tracing::debug!("Compiling source with rust-sitter...");
        
        // Parse with rust-sitter grammar (returns Document with Vec<Declaration>)
        match crate::schema::idl::parse::parse_schema(source) {
            Ok(document) => {
                // Extract declarations from Document (field 0 is Vec<Declaration>)
                let declarations = document.0;
                Self::from_declarations(declarations)
            }
            Err(diagnostics) => {
                panic!("{}", crate::diagnostics::render(&diagnostics[0], "<source>", source));
            }
        }
    }
}
