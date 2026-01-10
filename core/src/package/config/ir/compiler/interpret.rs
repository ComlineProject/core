// Standard Uses

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::ir::compiler::interpreter::IncrementalInterpreter;
use crate::schema::ir::compiler::Compile; // for from_declarations

// External Uses
use eyre::Result;

pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    for schema_context in project_context.schema_contexts.iter() {
        let declarations = { schema_context.borrow().declarations.clone() };

        let mut frozen_units = IncrementalInterpreter::from_declarations(declarations);

        // Inject Namespace unit
        let namespace = schema_context.borrow().namespace_joined();
        frozen_units.insert(
            0,
            crate::schema::ir::frozen::unit::FrozenUnit::Namespace(namespace),
        );

        *schema_context.borrow().frozen_schema.borrow_mut() = Some(frozen_units);
        schema_context.borrow().compile_state.borrow_mut().complete = true;
    }

    Ok(())
}
