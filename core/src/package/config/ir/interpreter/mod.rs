// Relative Modules
pub mod report;
pub mod interpret;
pub mod freezing;

// Standard Uses
use std::path::Path;
use std::rc::Rc;

// Crate Uses
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

// #[allow(unused)]
// impl Compile for ProjectInterpreter {
//    type Output = Result<ProjectContext>;
//    
//    // ... Removed ...
// }

// Non-trait method
// Non-trait method
impl ProjectInterpreter {
    pub fn from_config_source(source: &str) -> Result<ProjectContext> {
        let congregation = crate::package::config::idl::grammar::parse(source)
            .map_err(|e| eyre::eyre!("Parse error: {:?}", e))?;
            
        Ok(ProjectContext::with_config(congregation))
    }

    pub fn from_origin(origin: &Path) -> Result<ProjectContext> {
        let source = std::fs::read_to_string(origin)
            .map_err(|e| eyre::eyre!("Failed to read file {:?}: {}", origin, e))?;
            
        let mut context = Self::from_config_source(&source)?;
        // Update origin since from_config_source sets generic Virtual origin
        context.origin = crate::package::config::ir::context::Origin::Disk(origin.to_path_buf());
        
        context.config_frozen = Some(interpret::interpret_context(&context)
             .map_err(|e| eyre::eyre!("{:?}", e))?);
        
        Ok(context)
    }
}

