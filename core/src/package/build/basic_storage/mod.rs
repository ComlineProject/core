// Relative Modules
// Relative Modules
pub mod package;

// Standard Uses
use std::path::Path;
use std::fs::File;
use std::io::Write;

// Crate Uses
use crate::package::build::basic_storage_project;
use crate::package::build::basic_storage_schema;
use crate::package::config::ir::context::ProjectContext;

// External Uses
use eyre::Result;


pub fn process_changes(
    project_path: &Path, latest_project: &ProjectContext
) -> Result<()> {
    let frozen_path = project_path.join(".frozen");

    check_frozen_data_integrity(project_path)?;
    let package_path = frozen_path.join("package/");

    let index_path = package_path.join("index");

    let versions_path = package_path.join("versions/");

    let previous_version = basic_storage_project::get_latest_version(
        &frozen_path
    ).unwrap();
    let previous_version_path = versions_path.join(previous_version.to_string());

    let previous_project =
        basic_storage_project::deserialize::all_from_origin(&previous_version_path)
            .unwrap();

    let previous_schemas =
        basic_storage_schema::deserialize::all_from_version_frozen(&previous_version_path)?;

    // Collect latest schemas for comparison
    let mut latest_schemas = vec![];
    for schema_ctx in &latest_project.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        if let Some(frozen) = frozen_ref.as_ref() {
            latest_schemas.push(frozen.clone());
        }
    }

    let bump = package::check_difference(&previous_schemas, &latest_schemas);
    let mut latest_version = previous_version.clone();

    match bump {
        package::VersionBump::Major => latest_version.major += 1,
        package::VersionBump::Minor => latest_version.minor += 1,
        package::VersionBump::Patch => latest_version.patch += 1,
        package::VersionBump::None => {
            // No changes detected? Maybe only metadata? Or config changed?
            // For now, if no schema diff, we check project diff? 
            // Or default to patch if we run this (implies user wants to publish).
            // Let's assume Patch for now if "None" but we were asked to process changes.
            println!("No schema changes detected."); 
            // Return early if we want to prevent empty publications, 
            // OR bump patch if force-publishing.
            // Sticking to Patch for "something happened" default.
            latest_version.patch += 1; 
        }
    }

    // Reset lower fields on bump
    if bump == package::VersionBump::Major {
        latest_version.minor = 0;
        latest_version.patch = 0;
    } else if bump == package::VersionBump::Minor {
        latest_version.patch = 0;
    }

    let latest_version_path = versions_path.join(latest_version.to_string());
    std::fs::create_dir_all(&latest_version_path)?;

    package::freeze_and_compare_packages(
        &previous_project, &previous_schemas,
        &latest_project,
        &latest_version_path
    )?;

    File::options().append(true).open(&index_path)?.write(
        format!("\n{}", latest_version.to_string()).as_ref()
    )?;

    Ok(())
}

#[allow(unused)]
fn check_frozen_data_integrity(package_path: &Path) -> Result<()> {
    // TODO: Check integrity of the frozen contents, if they are valid,
    //       if something is broken, etc

    // todo!()
    Ok(())
}

#[allow(unused)]
pub(crate) fn process_initial_freezing(
    project_path: &Path, latest_project: &ProjectContext
) -> Result<()> {
    let frozen_path = project_path.join(".frozen/");

    /*
    let config_frozen = latest_project.config_frozen.as_ref().unwrap();
    let version = semver::Version::parse(
        frozen_project::version(config_frozen).unwrap()
    )?;

    if version.to_string() != MINIMUM_VERSION {
        bail!(
            "Initial version before freezing the package should be '{MINIMUM_VERSION}',\
             please change it in the package configuration"
        );
    }
    */

    package::freeze_project(
        &latest_project, &project_path
    )
}
