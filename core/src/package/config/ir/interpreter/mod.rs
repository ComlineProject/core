// Relative Modules
pub mod report;
pub mod interpret;
pub mod freezing;

// Standard Uses
use std::path::Path;
use std::rc::Rc;

// Crate Uses
// TODO: Re-implement with rust-sitter parser
// use crate::package::config::idl::parser_new;

// Local Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::ast::unit::*;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::Compile;

// External Uses
use eyre::{Result, eyre};


#[allow(unused)]
pub struct ProjectInterpreter {
    context: ProjectContext
}

#[allow(unused)]
impl Compile for ProjectInterpreter {
    type Output = Result<ProjectContext>;

    fn from_declarations(declarations: Vec<Declaration>) -> Self::Output {
        // TODO: Implement direct interpretation of rust-sitter types
        todo!("ProjectInterpreter::from_declarations - direct rust-sitter integration")
    }

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output {
        // Legacy implementation
        todo!()
    }

    fn from_sourced_whole(sourced: SourcedWholeRc) -> Self::Output {
        // TODO: Type mismatch - sourced is schema::idl::ast::unit::SourcedWholeRc
        // but with_config expects package::config::idl::ast::SourcedWhole
        // These are different AST types!
        todo!("from_sourced_whole - needs migration to new types")
    }

    fn from_source(source: &str) -> Self::Output {
        println!("Compiling source: {}", source);
        // TODO: Re-implement with rust-sitter parser
        unimplemented!("from_source not yet implemented with rust-sitter")
        // let ast = parser_new::parse_source(
        //     source.to_owned(), "".to_owned()
        // ).unwrap();
        // Self::from_sourced_whole(ast)
    }
}

// Non-trait method
impl ProjectInterpreter {
    pub fn from_origin(origin: &Path) -> Result<ProjectContext> {
        // TODO: Re-implement with rust-sitter parser
        unimplemented!("from_origin not yet implemented with rust-sitter")
        // let sourced = parser_new::from_path(origin).unwrap();
        // let mut context = ProjectContext::with_config_from_origin(
        //     Origin::Disk(origin.to_path_buf()), sourced
        // );
        // context.config_frozen = Some(interpret::interpret_context(&context)
        //     .map_err(|e| eyre!("{:?}", e))?);
        // Ok(context)
    }
}

