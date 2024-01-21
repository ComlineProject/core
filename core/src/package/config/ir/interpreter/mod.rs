// Relative Modules
pub mod report;
pub mod interpret;
pub mod freezing;

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::idl::parser_new;
use crate::package::config::idl::ast::{ASTUnit, SourcedWhole};
use crate::package::config::ir::context::{Origin, ProjectContext};
use crate::package::config::ir::compiler::Compile;

// External Uses
use eyre::{Result, eyre};


#[allow(unused)]
pub struct ProjectInterpreter {
    context: ProjectContext
}

#[allow(unused)]
impl Compile for ProjectInterpreter {
    type Output = Result<ProjectContext>;

    fn from_ast(ast: Vec<ASTUnit>) -> Self::Output {
        todo!()
    }

    fn from_sourced_whole(sourced: SourcedWhole) -> Self::Output {
        let mut context = ProjectContext::with_config(sourced);
        context.config_frozen = Some(interpret::interpret_context(&context)
            .map_err(|e| eyre!("{:?}", e))?);

        Ok(context)
    }

    fn from_source(source: &str) -> Self::Output {
        println!("Compiling source: {}", source);
        let ast = parser_new::parse_source(
            source.to_owned(), "".to_owned()
        ).unwrap();

        Self::from_sourced_whole(ast)
    }

    fn from_origin(origin: &Path) -> Self::Output {
        let sourced = parser_new::from_path(origin).unwrap();
        let mut context = ProjectContext::with_config_from_origin(
            Origin::Disk(origin.to_path_buf()), sourced
        );
        context.config_frozen = Some(interpret::interpret_context(&context)
            .map_err(|e| eyre!("{:?}", e))?);

        Ok(context)
    }
}
