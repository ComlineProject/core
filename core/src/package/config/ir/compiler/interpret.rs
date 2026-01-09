// Standard Uses

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::ir::compiler::interpreter::IncrementalInterpreter;
use crate::schema::ir::compiler::Compile; // for from_declarations

// External Uses
use eyre::Result;


pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    for schema_context in project_context.schema_contexts.iter() {
        let declarations = {
            schema_context.borrow().declarations.clone()
        };
        
        let frozen_units = IncrementalInterpreter::from_declarations(declarations);
        
        *schema_context.borrow().frozen_schema.borrow_mut() = Some(frozen_units);
    }

    Ok(())
}

