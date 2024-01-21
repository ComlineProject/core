// Relative Modules
mod package;
pub(crate) mod schema;

// Standard Uses
use std::path::Path;

// Crate Uses
use crate::project::ir::context::ProjectContext;
use crate::project::ir::frozen::FrozenUnit;
use crate::project::ir::diff as project_diff;

// External Uses
use eyre::Result;


#[allow(unused)]
pub fn process_changes(
    project_path: &Path,
    previous_project: &[FrozenUnit], latest_project: &ProjectContext
) -> Result<()> {
    project_diff::differ(
        previous_project, latest_project.config_frozen.as_ref().unwrap(),
        true, true
    );

    todo!()
}
