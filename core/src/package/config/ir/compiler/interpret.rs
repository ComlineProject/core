// Standard Uses
use std::rc::Rc;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::ir::compiler::interpreter::{meta_stage, object_stage};

// External Uses
use eyre::{Result, eyre};


pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    for schema_context in project_context.schema_contexts.iter() {
        meta_stage::compile_schema_metadata(
            Rc::clone(schema_context), project_context
        ).map_err(|e| eyre!("{}", e))?;
    }

    for schema_context in project_context.schema_contexts.iter() {
        object_stage::compile_schema(
            Rc::clone(schema_context),
            project_context
        ).map_err(|e| eyre!("{}", e))?;
    }

    Ok(())
}

