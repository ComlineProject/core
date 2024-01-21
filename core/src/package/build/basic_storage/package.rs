// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    basic_storage as basic_storage_project,
    FrozenUnit as ProjectFrozenUnit,
    MINIMUM_VERSION
};
use crate::schema::ir::frozen::{
    basic_storage as basic_storage_schema,
    unit::{FrozenUnit as SchemaFrozenUnit}
};

// External Uses
use eyre::{Context, Result};


pub(crate) fn freeze_project(
    latest_project_ctx: &ProjectContext, package_path: &Path
) -> Result<()> {
    let frozen_path = package_path.join(".frozen/");

    if frozen_path.exists() { std::fs::remove_dir_all(&frozen_path)? }
    std::fs::create_dir(&frozen_path)?;

    let dependencies_path = frozen_path.join("dependencies/");
    std::fs::create_dir(&dependencies_path)?;

    let frozen_package_path = frozen_path.join("package/");
    std::fs::create_dir(&frozen_package_path)?;

    let index_path = frozen_package_path.join("index");
    std::fs::write(index_path, MINIMUM_VERSION)?;

    let versions_path = frozen_package_path.join("versions/");
    std::fs::create_dir(&versions_path)?;

    let version_path = versions_path.join(MINIMUM_VERSION.to_owned() + "/");
    std::fs::create_dir(&version_path)?;

    let config_path = version_path.join("config");
    let frozen_project_processed = basic_storage_project::serialize::to_processed(
        latest_project_ctx.config_frozen.as_ref().unwrap()
    );
    std::fs::write(config_path, frozen_project_processed)?;

    let schemas_path = version_path.join("schemas/");
    std::fs::create_dir(&schemas_path)?;

    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let compile_state = schema_ref.compile_state.borrow();

        if !compile_state.complete {
            panic!("Cannot freeze schema because it is not marked as compiled")
        }

        let frozen_meta = basic_storage_schema::serialize::to_processed(
            schema_ref.frozen_schema.as_ref().unwrap()
        );
        let schema_path = schemas_path.join(&schema_ref.namespace_joined());

        std::fs::write(schema_path, frozen_meta)?;
    }

    Ok(())
}


pub(crate) fn freeze_and_compare_packages(
    previous_project: &[ProjectFrozenUnit], previous_schemas: &[Vec<SchemaFrozenUnit>],
    latest_project_ctx: &ProjectContext,
    latest_version_path: &Path
) -> Result<()> {
    let config_path = latest_version_path.join("config");
    let frozen_project_processed = basic_storage_project::serialize::to_processed(
        latest_project_ctx.config_frozen.as_ref().unwrap()
    );
    std::fs::write(config_path, frozen_project_processed)?;

    let schemas_path = latest_version_path.join("schemas");
    std::fs::create_dir_all(&schemas_path).with_context(|| {
        format!("Could not create frozen schemas directory at '{}'", schemas_path.display())
    })?;

    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let compile_state = schema_ref.compile_state.borrow();

        if !compile_state.complete {
            panic!("Cannot freeze schema because it is not marked as compiled")
        }

        let frozen_meta = basic_storage_schema::serialize::to_processed(
            schema_ref.frozen_schema.as_ref().unwrap()
        );

        let schema_path = schemas_path.join(&schema_ref.namespace_as_path());

        std::fs::create_dir_all(&schema_path.parent().unwrap()).with_context(|| {
            format!(
                "Could not create frozen schema parent directories at '{}",
                schema_path.parent().unwrap().display()
            )
        })?;

        std::fs::write(&schema_path, frozen_meta).with_context(|| {
            format!("Could not write frozen schema to path '{}'", schema_path.display())
        })?;
    }

    Ok(())
}

#[allow(unused)]
pub(crate) fn check_difference(
    previous: &[ProjectFrozenUnit], latest: &[ProjectFrozenUnit]
) -> Result<()> {
    todo!()
}
