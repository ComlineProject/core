// Relative Modules
pub(crate) mod package;

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

    // TODO: This is a temporary version bumper arrangement, should be much more elaborate on CAS
    let mut latest_version = previous_version.clone();
    latest_version.minor += 1;

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
