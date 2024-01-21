// Standard Uses
use std::path::Path;

// Crate Uses
use crate::project::ir::frozen::FrozenUnit;
use crate::project::ir::context::ProjectContext;

// External Uses
use eyre::Result;


#[allow(unused)]
pub(crate) fn freeze_project(
    previous_project_ctx: ProjectContext,
    latest_project_ctx: &ProjectContext,
    base_path: &Path
) -> Result<()> {
    let schemas_path = base_path.join(".frozen/package/schemas");

    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let compile_state = schema_ref.compile_state.borrow();

        if !compile_state.complete {
            panic!("Cannot freeze schema because it is not marked as compiled")
        }

        let frozen_meta = || {
            if let Some(other_schema)
                = previous_project_ctx.find_schema_by_import(&schema_ref.name)
            {
                let other_schema_ref = other_schema.borrow();
                return super::schema::check_difference(
                    schema_ref.frozen_schema.as_ref().unwrap(),
                    other_schema_ref.frozen_schema.as_ref().unwrap(),
                )
            }
            else {


                Ok(vec![])
            }
        };
        let frozen_meta = frozen_meta();

        let schema_path = schemas_path.join(&schema_ref.name);
    }

    Ok(())
}

#[allow(unused)]
pub(crate) fn check_difference(
    previous: &[FrozenUnit], latest: &[FrozenUnit]
) -> Result<()> {
    todo!()
}
